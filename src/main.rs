use std::io::{self, Write};
use std::net::Ipv4Addr;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, LeaveAlternateScreen},
};
use tui::{backend::CrosstermBackend, Terminal};
use ureq;

mod ip_iter;
mod ui;

pub const UI_FPS: usize = 10;
pub const THREAD_COUNT: usize = 100;
pub const IP_COUNT: usize = 4_294_967_296; // 256^4

enum Message {
    IpCheck(Ipv4Addr),
    IpFail(Ipv4Addr),
    ThreadExit(usize),
}

fn main() {
    let ips = ip_iter::IpIter::new().into_iter();
    let mspf = ((UI_FPS as f32).recip() * 1000.0) as u64;

    let ip_count_og = Arc::new(AtomicUsize::new(0));
    let threads_count = Arc::new(AtomicUsize::new(THREAD_COUNT));
    let events_og = Arc::new(RwLock::new(vec!["Starting".to_owned()]));
    let (tx, rx) = crossbeam_channel::unbounded();
    println!("Loading...");

    let events = events_og.clone();
    let ip_count = ip_count_og.clone();
    thread::spawn(move || {
        for i in rx {
            match i {
                Message::IpCheck(_) => {
                    ip_count.fetch_add(1, Ordering::Relaxed);
                }
                Message::IpFail(_) => {
                    ip_count.fetch_add(1, Ordering::Relaxed);
                }
                Message::ThreadExit(i) => {
                    threads_count.fetch_sub(1, Ordering::Relaxed);
                    events.write().unwrap().push(format!("Thread Exit [{}]", i));
                }
            };
        }
    });

    for (ti, e) in (0..THREAD_COUNT).enumerate() {
        let tx = tx.clone();
        let ip_iter = ips.skip(ti * (IP_COUNT / THREAD_COUNT));
        let mut ip_stop_index = ((ti + 1) * (IP_COUNT / THREAD_COUNT)) - 1;
        if e == THREAD_COUNT {
            ip_stop_index = IP_COUNT;
        }

        thread::spawn(move || {
            for (i, e) in ip_iter.enumerate() {
                if i >= ip_stop_index {
                    tx.send(Message::ThreadExit(ti)).unwrap();
                    break;
                }

                let res = match ureq::get(&format!("http://{}/", e))
                    .timeout(Duration::from_millis(100))
                    .call()
                {
                    Ok(i) => i,
                    Err(_) => {
                        tx.send(Message::IpCheck(e.to_ip_addr())).unwrap();
                        continue;
                    }
                };
                tx.send(Message::IpCheck(e.to_ip_addr())).unwrap()
            }
        });
    }

    enable_raw_mode().unwrap();
    let mut stdout = io::stdout();
    stdout
        .write(&Clear(ClearType::All).to_string().as_bytes())
        .unwrap();
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).unwrap();

    loop {
        let start = Instant::now();

        terminal
            .draw(|f| ui::ui(f, events_og.clone(), ip_count_og.clone()))
            .unwrap();

        let timeout =
            Duration::from_millis(mspf.saturating_sub(start.elapsed().as_millis() as u64));
        if crossterm::event::poll(timeout).unwrap() {
            if let Event::Key(key) = event::read().unwrap() {
                match key.code {
                    KeyCode::Esc => break,
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode().unwrap();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).unwrap();
    terminal.show_cursor().unwrap();
}
