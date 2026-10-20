//! Drives the calculator Extension the way the Daemon does: over its stdio,
//! with its cache in a temporary directory and its exchange rates from a fake
//! endpoint on localhost.

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, SystemTime};

use kotodama_protocol::*;
use tempfile::TempDir;

/// Long enough for the Extension to answer, or to fetch rates.
const WAIT: Duration = Duration::from_secs(10);

const RATES: &str =
    r#"{"amount":1.0,"base":"EUR","date":"2026-10-19","rates":{"PLN":4.0,"USD":1.25}}"#;

struct Calculator {
    _cache: TempDir,
    child: Child,
    writer: ChannelWriter<ChildStdin>,
    received: Receiver<FromExtension>,
    next_query: u64,
}

impl Calculator {
    /// Starts the Extension with its cache in `cache` and sends it `config`.
    fn start(cache: TempDir, config: Table) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_kotodama-calculator"))
            .env("XDG_CACHE_HOME", cache.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (stdout, stdin) = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
        let channel = Channel::handshake(stdout, stdin, &Hello::new(&WireFormat::ALL)).unwrap();
        let (mut reader, writer) = channel.split();
        let (sender, received) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(Some(message)) = reader.recv::<FromExtension>() {
                if sender.send(message).is_err() {
                    break;
                }
            }
        });
        let mut calculator = Calculator {
            _cache: cache,
            child,
            writer,
            received,
            next_query: 1,
        };
        calculator.send(ToExtension::Config(config));
        calculator
    }

    fn send(&mut self, message: ToExtension) {
        self.writer.send(&message).unwrap();
    }

    /// Sends `query` as a Live query and returns its id.
    fn ask(&mut self, query: &str, prefix: Option<&str>) -> u64 {
        let id = self.next_query;
        self.next_query += 1;
        self.send(ToExtension::LiveQuery {
            id,
            query: query.into(),
            prefix: prefix.map(str::to_owned),
        });
        id
    }

    /// The results for the Live query `id`, which must come all at once.
    fn results(&self, id: u64) -> Vec<LiveResult> {
        match self.received.recv_timeout(WAIT) {
            Ok(FromExtension::LiveResults {
                live_query,
                results,
                done: true,
            }) if live_query == id => results,
            other => panic!("expected the results of {id}, got {other:?}"),
        }
    }

    /// Asks `query` and returns the title of its answer, if there is one.
    fn answer(&mut self, query: &str) -> Option<String> {
        let id = self.ask(query, None);
        let results = self.results(id);
        assert!(results.len() <= 1, "{results:?}");
        results.into_iter().next().map(|result| {
            assert!(result.answer);
            result.item.title
        })
    }

    fn run(&mut self, id: &str, action: &str) -> Outcome {
        self.send(ToExtension::RunAction {
            invocation: 7,
            action: action.into(),
            item: ItemRef {
                id: id.into(),
                kind: ItemKind::Namespaced(NamespacedKind::new("calculator.answer").unwrap()),
            },
            arguments: BTreeMap::new(),
        });
        match self.received.recv_timeout(WAIT) {
            Ok(FromExtension::ActionDone {
                invocation: 7,
                outcome,
            }) => outcome,
            other => panic!("expected action_done, got {other:?}"),
        }
    }
}

impl Drop for Calculator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A fake rates endpoint: answers every request with [`RATES`], and reports
/// each request's first line.
struct Endpoint {
    url: String,
    requests: Receiver<String>,
}

impl Endpoint {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/latest", listener.local_addr().unwrap());
        let (sender, requests) = mpsc::channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut first = String::new();
                let _ = reader.read_line(&mut first);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{RATES}",
                    RATES.len()
                );
                let _ = sender.send(first.trim().to_owned());
            }
        });
        Endpoint { url, requests }
    }

    fn requested(&self) -> bool {
        self.requests.recv_timeout(WAIT).is_ok()
    }

    fn quiet(&self) {
        if let Ok(request) = self.requests.recv_timeout(Duration::from_millis(300)) {
            panic!("expected no request, got {request}");
        }
    }
}

fn config(pairs: &[(&str, Value)]) -> Table {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn online(endpoint: &Endpoint) -> Table {
    config(&[("rates_url", Value::String(endpoint.url.clone()))])
}

fn offline(endpoint: &Endpoint) -> Table {
    config(&[
        ("rates_url", Value::String(endpoint.url.clone())),
        ("network", Value::Bool(false)),
    ])
}

fn rates_file(cache: &TempDir) -> PathBuf {
    cache.path().join("kotodama/calculator/rates.json")
}

/// A cache holding [`RATES`], fetched `age` ago.
fn cached(age: Duration) -> TempDir {
    let cache = tempfile::tempdir().unwrap();
    let file = rates_file(&cache);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, RATES).unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
    cache
}

#[test]
fn math_is_answered_with_an_answer_that_copies() {
    let mut calc = Calculator::start(tempfile::tempdir().unwrap(), Table::new());
    let id = calc.ask("42*2", None);
    let results = calc.results(id);
    assert_eq!(results.len(), 1);
    let result = &results[0];
    assert!(result.answer);
    assert_eq!(result.item.title, "84");
    assert_eq!(result.item.subtitle.as_deref(), Some("42*2"));
    assert_eq!(result.item.kind.name(), "calculator.answer");
    let actions: Vec<&str> = result.item.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(actions, ["copy", "copy-expression"]);

    let item = result.item.id.clone();
    assert_eq!(
        calc.run(&item, "copy"),
        Outcome::Copy(Clipboard::Text("84".into()))
    );
    assert_eq!(
        calc.run(&item, "copy-expression"),
        Outcome::Copy(Clipboard::Text("42*2 = 84".into()))
    );
}

#[test]
fn what_is_not_math_gets_no_results() {
    let mut calc = Calculator::start(tempfile::tempdir().unwrap(), Table::new());
    assert_eq!(calc.answer("fi"), None);
    assert_eq!(calc.answer("42"), None);
    assert_eq!(calc.answer("2 +"), None);
}

#[test]
fn the_prefix_answers_what_evaluates_without_looking_like_math() {
    let mut calc = Calculator::start(tempfile::tempdir().unwrap(), Table::new());
    let id = calc.ask(" pi", Some("="));
    let titles: Vec<String> = calc.results(id).into_iter().map(|r| r.item.title).collect();
    assert_eq!(titles, ["3.1415926536"]);
}

#[test]
fn dates_and_times_are_answered() {
    let mut calc = Calculator::start(tempfile::tempdir().unwrap(), Table::new());
    assert_eq!(calc.answer("days until tomorrow").as_deref(), Some("1 day"));
    assert!(
        calc.answer("now in Tokyo")
            .is_some_and(|time| time.contains("JST"))
    );
}

#[test]
fn rates_are_fetched_only_once_currency_is_asked_for_and_then_cached() {
    let endpoint = Endpoint::start();
    let cache = tempfile::tempdir().unwrap();
    let file = rates_file(&cache);
    let mut calc = Calculator::start(cache, online(&endpoint));
    assert_eq!(calc.answer("1+1").as_deref(), Some("2"));
    assert_eq!(
        calc.answer("5 km in mi").as_deref(),
        Some("3.1068559612 mi")
    );
    endpoint.quiet();

    assert_eq!(calc.answer("100 usd to eur").as_deref(), Some("80 EUR"));
    assert!(endpoint.requested());
    assert_eq!(fs::read_to_string(&file).unwrap(), RATES);

    assert_eq!(calc.answer("10 eur to pln").as_deref(), Some("40 PLN"));
    endpoint.quiet();
}

#[test]
fn a_query_given_up_while_rates_are_fetched_gets_no_results() {
    let endpoint = Endpoint::start();
    let mut calc = Calculator::start(tempfile::tempdir().unwrap(), online(&endpoint));
    let id = calc.ask("100 usd to eur", None);
    calc.send(ToExtension::CancelLiveQuery { id });
    assert!(endpoint.requested());
    assert_eq!(calc.answer("1+2").as_deref(), Some("3"));
}

#[test]
fn rates_cached_today_are_used_without_fetching() {
    let endpoint = Endpoint::start();
    let mut calc = Calculator::start(cached(Duration::from_secs(60 * 60)), online(&endpoint));
    assert_eq!(calc.answer("100 usd to eur").as_deref(), Some("80 EUR"));
    endpoint.quiet();
}

#[test]
fn rates_older_than_a_day_are_used_and_fetched_again() {
    let endpoint = Endpoint::start();
    let mut calc = Calculator::start(cached(Duration::from_secs(25 * 60 * 60)), online(&endpoint));
    assert_eq!(calc.answer("100 usd to eur").as_deref(), Some("80 EUR"));
    assert!(endpoint.requested());
}

#[test]
fn with_the_network_off_cached_rates_still_answer() {
    let endpoint = Endpoint::start();
    let mut calc = Calculator::start(
        cached(Duration::from_secs(30 * 24 * 60 * 60)),
        offline(&endpoint),
    );
    assert_eq!(calc.answer("100 usd to eur").as_deref(), Some("80 EUR"));
    endpoint.quiet();
}

#[test]
fn with_the_network_off_and_nothing_cached_currency_gets_no_results() {
    let endpoint = Endpoint::start();
    let mut calc = Calculator::start(tempfile::tempdir().unwrap(), offline(&endpoint));
    assert_eq!(calc.answer("100 usd to eur"), None);
    endpoint.quiet();
}
