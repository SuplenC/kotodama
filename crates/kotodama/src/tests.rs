use super::*;

fn args(line: &str) -> Vec<String> {
    line.split(' ').map(String::from).collect()
}

#[test]
fn open_takes_a_query_and_a_view_in_any_order() {
    assert_eq!(
        parse(&args("open --view clipboard --query fir")),
        Ok(FromFrontEnd::Open {
            query: "fir".into(),
            view: Some("clipboard".into()),
        })
    );
    assert_eq!(
        parse(&args("open")),
        Ok(FromFrontEnd::Open {
            query: String::new(),
            view: None,
        })
    );
}

#[test]
fn open_in_an_app_types_its_word_before_the_query() {
    assert_eq!(
        parse(&args("open --app emoji")),
        Ok(FromFrontEnd::Open {
            query: "emoji ".into(),
            view: None,
        })
    );
    assert_eq!(
        parse(&args("open --query fire --app emoji")),
        Ok(FromFrontEnd::Open {
            query: "emoji fire".into(),
            view: None,
        })
    );
}

#[test]
fn prefix_is_still_read_as_app() {
    assert_eq!(
        parse(&args("open --prefix emoji")),
        Ok(FromFrontEnd::Open {
            query: "emoji ".into(),
            view: None,
        })
    );
}

#[test]
fn a_flag_without_a_value_or_an_unknown_one_is_an_error() {
    assert!(parse(&args("open --query")).is_err());
    assert!(parse(&args("open --size 3")).is_err());
    assert!(parse(&args("toggle now")).is_err());
    assert!(parse(&[]).is_err());
}

/// What the command reads as the answer from a Daemon that does `daemon`.
fn answered_by(daemon: impl FnOnce(&mut Channel<UnixStream, UnixStream>) + Send + 'static) -> Result<u32, SendError> {
    let (ours, theirs) = UnixStream::pair().unwrap();
    let peer = std::thread::spawn(move || {
        let reader = theirs.try_clone().unwrap();
        let mut channel = Channel::handshake(reader, theirs, &Hello::new(&WireFormat::ALL)).unwrap();
        daemon(&mut channel);
    });
    let reader = ours.try_clone().unwrap();
    let mut channel = Channel::handshake(reader, ours, &Hello::new(&WireFormat::ALL)).unwrap();
    let answer = answer(&mut channel);
    peer.join().unwrap();
    answer
}

#[test]
fn the_answer_is_how_many_menus_the_daemon_reached() {
    let answer = answered_by(|daemon| {
        daemon.send(&ToFrontEnd::Status { down: Vec::new() }).unwrap();
        daemon.send(&ToFrontEnd::Reached { menus: 1 }).unwrap();
    });

    assert_eq!(answer.unwrap(), 1);
}

#[test]
fn a_request_to_close_while_waiting_is_answered_at_once() {
    let answer = answered_by(|daemon| {
        daemon.send(&ToFrontEnd::Close).unwrap();
        assert_eq!(daemon.recv::<FromFrontEnd>().unwrap(), Some(FromFrontEnd::Closed));
        daemon.send(&ToFrontEnd::Reached { menus: 0 }).unwrap();
    });

    assert_eq!(answer.unwrap(), 0);
}

#[test]
fn no_menu_says_how_to_start_one() {
    let message = SendError::NoMenu.to_string();

    assert!(message.starts_with("no menu is running"), "{message}");
    assert!(message.contains("programs.kotodama.frontEnd.enable"), "{message}");
}

#[test]
fn a_daemon_that_hangs_up_before_it_answers_is_an_error() {
    let answer = answered_by(|_| {});

    assert!(matches!(answer, Err(SendError::HungUp)), "{answer:?}");
}
