//! MIDI learn lifetime, independent of devices and the GUI event loop.
use std::sync::{
	Arc, Mutex,
	atomic::{AtomicBool, Ordering},
	mpsc::{Receiver, RecvTimeoutError},
};
use std::time::{Duration, Instant};

pub static CAPTURES: Coordinator = Coordinator::new();
pub struct Ticket {
	pub id: String,
	cancelled: AtomicBool,
}
impl Ticket {
	fn check(&self) -> Result<(), String> {
		if self.cancelled.load(Ordering::Acquire) {
			Err("MIDI note capture cancelled".into())
		} else {
			Ok(())
		}
	}
	pub fn receive<T>(&self, rx: &Receiver<T>, timeout: Duration) -> Result<T, String> {
		let deadline = Instant::now() + timeout;
		loop {
			self.check()?;
			let remaining = deadline.saturating_duration_since(Instant::now());
			if remaining.is_zero() {
				return Err("MIDI note capture timed out".into());
			}
			match rx.recv_timeout(remaining.min(Duration::from_millis(25))) {
				Ok(value) => {
					self.check()?;
					return Ok(value);
				}
				Err(RecvTimeoutError::Timeout) => {}
				Err(RecvTimeoutError::Disconnected) => return Err("MIDI input disconnected".into()),
			}
		}
	}
}
struct State {
	next: u64,
	current: Option<Arc<Ticket>>,
}
pub struct Coordinator {
	state: Mutex<State>,
	devices: Mutex<()>,
}
impl Coordinator {
	pub const fn new() -> Self {
		Self {
			state: Mutex::new(State { next: 0, current: None }),
			devices: Mutex::new(()),
		}
	}
	pub fn reserve(&self) -> Result<String, String> {
		let mut state = self.state.lock().map_err(|_| "MIDI capture state poisoned")?;
		state.next = state.next.checked_add(1).ok_or("MIDI capture identifier exhausted")?;
		if let Some(old) = state.current.take() {
			old.cancelled.store(true, Ordering::Release);
		}
		let id = state.next.to_string();
		state.current = Some(Arc::new(Ticket {
			id: id.clone(),
			cancelled: AtomicBool::new(false),
		}));
		Ok(id)
	}
	pub fn cancel(&self, id: &str) -> Result<(), String> {
		let mut state = self.state.lock().map_err(|_| "MIDI capture state poisoned")?;
		if state.current.as_ref().is_some_and(|ticket| ticket.id == id) {
			if let Some(ticket) = state.current.take() {
				ticket.cancelled.store(true, Ordering::Release);
			}
		}
		Ok(())
	}
	pub fn run<T>(&self, id: &str, work: impl FnOnce(&Ticket) -> Result<T, String>) -> Result<T, String> {
		let ticket = self
			.state
			.lock()
			.map_err(|_| "MIDI capture state poisoned")?
			.current
			.as_ref()
			.filter(|ticket| ticket.id == id)
			.cloned()
			.ok_or("MIDI note capture cancelled")?;
		struct Finish<'a>(&'a Coordinator, &'a str);
		impl Drop for Finish<'_> {
			fn drop(&mut self) {
				let _ = self.0.cancel(self.1);
			}
		}
		let _finish = Finish(self, id);
		// A replacement waits for the previous worker to drop its actual MIDI connections.
		let _devices = self.devices.lock().map_err(|_| "MIDI capture device lock poisoned")?;
		ticket.check()?;
		let value = work(&ticket)?;
		ticket.check()?;
		Ok(value)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::mpsc;
	#[test]
	fn cancelled_reservation_never_opens_devices() {
		let c = Coordinator::new();
		let id = c.reserve().unwrap();
		c.cancel(&id).unwrap();
		assert!(c.run::<()>(&id, |_| panic!("must not open")).is_err());
	}
	#[test]
	fn timeout_and_failure_release_reservation_for_retry() {
		let c = Coordinator::new();
		let id = c.reserve().unwrap();
		let (_tx, rx) = mpsc::channel::<u8>();
		assert!(
			c.run(&id, |t| t.receive(&rx, Duration::from_millis(5)))
				.unwrap_err()
				.contains("timed out")
		);
		let id = c.reserve().unwrap();
		assert!(c.run::<()>(&id, |_| Err("open failed".into())).is_err());
		let id = c.reserve().unwrap();
		assert_eq!(c.run(&id, |_| Ok(0)).unwrap(), 0);
	}
	#[test]
	fn replacement_waits_for_port_release_and_old_cancel_cannot_cancel_new() {
		let c = Arc::new(Coordinator::new());
		let old = c.reserve().unwrap();
		let (started_tx, started_rx) = mpsc::channel();
		let (released_tx, released_rx) = mpsc::channel();
		let worker = c.clone();
		let old_worker = old.clone();
		let join = std::thread::spawn(move || {
			worker.run(&old_worker, |ticket| {
				struct Port(std::sync::mpsc::Sender<()>);
				impl Drop for Port {
					fn drop(&mut self) {
						self.0.send(()).unwrap();
					}
				}
				let _port = Port(released_tx);
				let (_tx, rx) = mpsc::channel::<u8>();
				started_tx.send(()).unwrap();
				ticket.receive(&rx, Duration::from_secs(2))
			})
		});
		started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
		let next = c.reserve().unwrap();
		c.cancel(&old).unwrap();
		assert_eq!(
			c.run(&next, |_| {
				released_rx.try_recv().unwrap();
				Ok(7)
			})
			.unwrap(),
			7
		);
		assert!(join.join().unwrap().unwrap_err().contains("cancelled"));
	}
	#[test]
	fn cancelled_ticket_rejects_a_buffered_note() {
		let c = Coordinator::new();
		let id = c.reserve().unwrap();
		assert!(
			c.run(&id, |ticket| {
				let (tx, rx) = mpsc::channel();
				tx.send(0).unwrap();
				c.cancel(&id).unwrap();
				ticket.receive(&rx, Duration::from_secs(1))
			})
			.is_err()
		);
	}
}
