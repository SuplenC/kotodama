#!/usr/bin/env node
// A Kotodama Extension in TypeScript, speaking the protobuf Wire format of
// crates/kotodama-protocol/kotodama.proto. Node 23.6 or later runs it as it is,
// stripping the types; there is nothing to build or install.
//
// It answers Live queries: `ts 1700000000` shows that time as a date, and
// `ts 2024-02-29` that date as a timestamp. Enter copies the form picked;
// Ctrl+T types it into the window you came from.
//
// stdin and stdout carry the protocol, so nothing else may write to stdout:
// log to stderr. See README.md beside it, and docs/extensions.md.

import { Fields, Writer } from "./protobuf.ts";

// The protocol version this Extension is written for. A newer Daemon talks to
// it in 1.0's terms; an older one can't.
const MAJOR = 1;
const MINOR = 0;

// The variants of ToExtension and FromExtension, and of the messages inside
// them, numbered as in kotodama.proto.
const TO_EXTENSION = { liveQuery: 2, runAction: 4, rejected: 9 } as const;
const FROM_EXTENSION = { liveResults: 3, actionDone: 4 } as const;
const OUTCOME = { type: 3, copy: 5, failed: 6 } as const;

function log(message: string): void {
  process.stderr.write(`timestamp: ${message}\n`);
}

/** Sends one frame: the payload's length in bytes, a newline, the payload. */
function send(payload: Uint8Array): void {
  process.stdout.write(Buffer.concat([Buffer.from(`${payload.length}\n`), payload]));
}

/** The forms of one moment the Live results offer, best first. */
function forms(moment: Date): { form: string; text: string }[] {
  return [
    { form: "ISO 8601, UTC", text: moment.toISOString() },
    { form: "Unix time, seconds", text: String(Math.floor(moment.getTime() / 1000)) },
    { form: "HTTP date", text: moment.toUTCString() },
  ];
}

/** The moment a query names: nothing is now, a number a Unix time, anything else a date. */
function parse(query: string): Date | undefined {
  const text = query.trim();
  if (text === "") return new Date();
  if (/^-?\d+$/.test(text)) {
    const n = Number(text);
    // Past the year 5138 in seconds, it reads as milliseconds.
    return new Date(Math.abs(n) > 1e11 ? n : n * 1000);
  }
  const ms = Date.parse(text);
  return Number.isNaN(ms) ? undefined : new Date(ms);
}

function action(id: string, title: string, shortcut?: string): Writer {
  const action = new Writer().string(1, id).string(2, title);
  return shortcut === undefined ? action : action.string(3, shortcut);
}

/** `live_results` for Live query `id`: one Item per form of the moment. */
function liveResults(id: bigint, query: string): Writer {
  const results = new Writer().varint(1, id);
  const moment = parse(query);
  if (moment !== undefined && !Number.isNaN(moment.getTime())) {
    forms(moment).forEach(({ form, text }, rank) => {
      // The Item's id is its text, so running an Action needs nothing
      // remembered.
      const item = new Writer()
        .string(1, text)
        .string(2, "timestamp.time")
        .string(8, text)
        .string(9, form)
        .message(13, action("copy", "Copy"))
        .message(13, action("type", "Type it", "ctrl+t"));
      results.message(2, new Writer().message(1, item).float(2, 1 - rank / 10));
    });
  }
  results.varint(3, true);
  return new Writer().message(FROM_EXTENSION.liveResults, results);
}

/** `action_done` for a `run_action`, with how it finished. */
function actionDone(run: Fields): Writer {
  const action = run.string(2);
  const text = run.message(3).string(1);
  const outcome = new Writer();
  if (action === "copy") {
    outcome.message(OUTCOME.copy, new Writer().string(1, text));
  } else if (action === "type") {
    outcome.message(OUTCOME.type, new Writer().string(1, text));
  } else {
    outcome.message(OUTCOME.failed, new Writer().string(1, `unknown action ${action}`));
  }
  const done = new Writer().varint(1, run.number(1)).message(2, outcome);
  return new Writer().message(FROM_EXTENSION.actionDone, done);
}

function handle(message: Fields): void {
  // A ToExtension holds one field: the variant that came.
  for (const variant of message.numbers()) {
    const body = message.message(variant);
    switch (variant) {
      case TO_EXTENSION.liveQuery:
        send(liveResults(body.number(1), body.string(2)).bytes());
        break;
      case TO_EXTENSION.runAction:
        send(actionDone(body).bytes());
        break;
      case TO_EXTENSION.rejected:
        log(`the Daemon rejected a message: ${body.string(1)}`);
        break;
      default:
      // The config, and anything a newer Daemon adds, are ignored.
    }
  }
}

// The handshake is always JSON: both sides send a hello at once. Offering
// protobuf settles on it, since the Daemon prefers it to JSON, which every
// side must offer too.
send(Buffer.from(JSON.stringify({ version: MAJOR, minor: MINOR, formats: ["protobuf", "json"] })));

let buffered = Buffer.alloc(0);
let greeted = false;
process.stdin.on("data", (chunk: Buffer) => {
  buffered = Buffer.concat([buffered, chunk]);
  for (;;) {
    const newline = buffered.indexOf(10);
    if (newline < 0) return;
    const length = Number(buffered.subarray(0, newline).toString());
    const end = newline + 1 + length;
    if (buffered.length < end) return;
    const payload = buffered.subarray(newline + 1, end);
    buffered = buffered.subarray(end);
    if (greeted) {
      handle(new Fields(payload));
      continue;
    }
    greeted = true;
    const daemon: { version: number; minor?: number } = JSON.parse(payload.toString());
    const minor = daemon.minor ?? 0;
    if (daemon.version < MAJOR || (daemon.version === MAJOR && minor < MINOR)) {
      log(`this Daemon speaks protocol ${daemon.version}.${minor}; update Kotodama`);
      process.exit(1);
    }
  }
});
process.stdin.on("end", () => process.exit(0));
