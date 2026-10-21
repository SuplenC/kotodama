// The protobuf Wire format for the messages this Extension sends and reads,
// field by field as crates/kotodama-protocol/kotodama.proto numbers them.
//
// A bigger Extension generates these from kotodama.proto instead, with
// protoc-gen-es (@bufbuild/protobuf) or ts-proto. This one writes the dozen
// fields it uses by hand, so it runs on Node alone, with nothing to install.

/** Writes one protobuf message: call a method per field, then `bytes`. */
export class Writer {
  #out: number[] = [];

  /** A `uint64`, `uint32` or `bool` field. A zero is left out, as protobuf does. */
  varint(field: number, value: bigint | number | boolean): this {
    const n = BigInt(value);
    if (n !== 0n) {
      this.#tag(field, 0);
      this.#varint(n);
    }
    return this;
  }

  /** A `float` field. */
  float(field: number, value: number): this {
    const bytes = new Uint8Array(4);
    new DataView(bytes.buffer).setFloat32(0, value, true);
    this.#tag(field, 5);
    this.#out.push(...bytes);
    return this;
  }

  /** A `string` field. */
  string(field: number, value: string): this {
    return this.bytesField(field, new TextEncoder().encode(value));
  }

  /** A field holding another message, such as one variant of a `oneof`. */
  message(field: number, message: Writer): this {
    return this.bytesField(field, message.bytes());
  }

  bytesField(field: number, value: Uint8Array): this {
    this.#tag(field, 2);
    this.#varint(BigInt(value.length));
    this.#out.push(...value);
    return this;
  }

  bytes(): Uint8Array {
    return Uint8Array.from(this.#out);
  }

  #tag(field: number, wireType: number): void {
    this.#varint(BigInt((field << 3) | wireType));
  }

  #varint(value: bigint): void {
    let n = value;
    while (n >= 0x80n) {
      this.#out.push(Number(n & 0x7fn) | 0x80);
      n >>= 7n;
    }
    this.#out.push(Number(n));
  }
}

/** A field as read: a number for varints, bytes for strings and messages. */
export type Field = bigint | Uint8Array;

/** A message's fields, by number, each as often as it came. */
export class Fields {
  #fields = new Map<number, Field[]>();

  constructor(bytes: Uint8Array) {
    let at = 0;
    const varint = (): bigint => {
      let value = 0n;
      for (let shift = 0n; ; shift += 7n) {
        const byte = bytes[at++];
        if (byte === undefined) throw new Error("a protobuf message ends inside a number");
        value |= BigInt(byte & 0x7f) << shift;
        if (byte < 0x80) return value;
      }
    };
    while (at < bytes.length) {
      const tag = Number(varint());
      const field = tag >> 3;
      let value: Field;
      switch (tag & 7) {
        case 0:
          value = varint();
          break;
        case 1:
          value = bytes.subarray(at, (at += 8));
          break;
        case 2: {
          const length = Number(varint());
          value = bytes.subarray(at, (at += length));
          break;
        }
        case 5:
          value = bytes.subarray(at, (at += 4));
          break;
        default:
          throw new Error(`unknown protobuf wire type ${tag & 7}`);
      }
      const seen = this.#fields.get(field) ?? [];
      seen.push(value);
      this.#fields.set(field, seen);
    }
  }

  /** The numbers of the fields present, as in which variant of a `oneof` came. */
  numbers(): number[] {
    return [...this.#fields.keys()];
  }

  /** A `uint64` or `bool` field, 0 when absent. */
  number(field: number): bigint {
    const value = this.#last(field);
    return typeof value === "bigint" ? value : 0n;
  }

  /** A `string` field, empty when absent. */
  string(field: number): string {
    const value = this.#last(field);
    return value instanceof Uint8Array ? new TextDecoder().decode(value) : "";
  }

  /** A message field, empty when absent. */
  message(field: number): Fields {
    const value = this.#last(field);
    return new Fields(value instanceof Uint8Array ? value : new Uint8Array());
  }

  /** A repeated message field, such as each entry of a `map`. */
  messages(field: number): Fields[] {
    return (this.#fields.get(field) ?? []).flatMap((value) =>
      value instanceof Uint8Array ? [new Fields(value)] : [],
    );
  }

  #last(field: number): Field | undefined {
    return this.#fields.get(field)?.at(-1);
  }
}
