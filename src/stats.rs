use colored::*;
use std::time::Instant;
use once_cell::sync::Lazy;
use std::sync::Mutex;

pub fn get_elapsed_string(elapsed: Instant) -> String {
	format!("[{}{}] ", format!("{:>4.1}", (elapsed.elapsed().as_millis() as f64 / 1000.0)).dimmed(), "s".dimmed()).to_string()
}

pub static GLOBAL_PASS_START_TIME: Lazy<Mutex<Instant>> = Lazy::new(|| Mutex::new(Instant::now()));
pub static GLOBAL_PASS_NUMBER: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));
pub fn set_global_pass_start_time(i: Instant, pass_number: u64) {
	let mut global_pass_start_time = GLOBAL_PASS_START_TIME.lock().unwrap();
	*global_pass_start_time=i.clone();
	let mut global_pass_number = GLOBAL_PASS_NUMBER.lock().unwrap();
	*global_pass_number=pass_number;
}
pub fn get_global_pass_start_time() -> Instant {
	let global_pass_start_time = GLOBAL_PASS_START_TIME.lock().unwrap();
	*global_pass_start_time
}

pub static MINER_ACCURACY_BUFFER: Lazy<Mutex<CircularBuffer>> = Lazy::new(|| Mutex::new(CircularBuffer::new(30)));
pub fn get_miner_accuracy() -> f64 {
	let miner_accuracy_buffer = MINER_ACCURACY_BUFFER.lock().unwrap();
	miner_accuracy_buffer.calculate_median()
}
pub fn record_miner_accuracy(accuracy: f64) {
	let mut miner_accuracy_buffer = MINER_ACCURACY_BUFFER.lock().unwrap();
	if accuracy>=-1000.0 && accuracy<=5_000_000.0 {
		miner_accuracy_buffer.insert(accuracy);
		println!("        Accuracy: {} {}\t\t\t[{} -> {} -> {}]", 
			format!("{:.0}", accuracy).green(), ("ms").dimmed(),
			format!("{:.0}", miner_accuracy_buffer.calculate_min()), 
			format!("{:.0}", miner_accuracy_buffer.calculate_median()).cyan(), 
			format!("{:.0}", miner_accuracy_buffer.calculate_max()).green(),
		);
	} else {
		println!("        Accuracy: {}{}\t{}", 
			format!("{:.0}", accuracy).green(), ("ms").dimmed(),
			format!("Ignored as outwith tolerance").yellow(),
		);
	}
}

// -------------------------------------
pub static NO_MORE_SUBMISSIONS: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::new(false));
pub fn set_no_more_submissions(the_state: bool) {
	let mut no_more_submissions = NO_MORE_SUBMISSIONS.lock().unwrap();
	*no_more_submissions=the_state;
	// println!("record_tx_started: {}ms ago", global_tx_start_time.elapsed().as_millis());
}
pub fn is_transaction_in_progress() -> bool {
	// no_proe_submissions==true => transaction in progress
	let no_more_submissions = NO_MORE_SUBMISSIONS.lock().unwrap();
	*no_more_submissions
}

pub static TX_TIME_BUFFER: Lazy<Mutex<CircularBuffer>> = Lazy::new(|| Mutex::new(CircularBuffer::new(120)));
pub static GLOBAL_TX_START_TIME: Lazy<Mutex<Instant>> = Lazy::new(|| Mutex::new(Instant::now()));
pub static GLOBAL_TX_OVERTIME: Lazy<Mutex<u64>> = Lazy::new(|| Mutex::new(0));
pub fn record_tx_started() {
	let mut global_tx_start_time = GLOBAL_TX_START_TIME.lock().unwrap();
	*global_tx_start_time=Instant::now();
	// println!("record_tx_started: {}ms ago", global_tx_start_time.elapsed().as_millis());
}
pub fn record_tx_complete() {
	let global_tx_start_time = GLOBAL_TX_START_TIME.lock().unwrap();
	let tx_time_ms = global_tx_start_time.elapsed().as_micros() as f64 / 1000000.0;
	let mut tx_time_buffer = TX_TIME_BUFFER.lock().unwrap();
	tx_time_buffer.insert(tx_time_ms);

	if tx_time_ms>10.0 {
		let mut global_tx_overtime = GLOBAL_TX_OVERTIME.lock().unwrap();
		*global_tx_overtime+=1;
	}
}

// Implement a circular buffer for calculating averages of last N values.
pub struct CircularBuffer {
	data: Vec<f64>,
	capacity: usize,
}
impl CircularBuffer {
	pub fn new(capacity: usize) -> Self {
		CircularBuffer {
			data: Vec::with_capacity(capacity),
			capacity,
		}
	}

	pub fn insert(&mut self, value: f64) {
		if self.data.len() >= self.capacity {
			self.data.remove(0); // Remove the oldest entry
		}
		self.data.push(value);
	}

	pub fn _num_entries(&self) -> u128 {
		self.data.len() as u128
	}

	// pub fn latest_entry(&self) -> f64 {
	// 	if self.data.len() == 0 {
	// 		0.0
	// 	} else {
	// 		self.data[self.data.len()-1]
	// 	}
	// }

	// pub fn get_last_entries(&self) -> Vec<f64> {
    //     let start = self.data.len().saturating_sub(7);
    //     self.data[start..].to_vec()
    // }

	pub fn calculate_median(&self) -> f64 {
        if self.data.is_empty() {
            return 0.0;
        }
        let mut sorted = self.data.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
		let mid = sorted.len() / 2;
		if sorted.len()<7 {
			// Return median on short lists
			if sorted.len() % 2 == 0 {
				return (sorted[mid - 1] + sorted[mid]) / 2.0;
			} else {
				return sorted[mid];
			}
		}

		// Return average around the median with >7 values
		let min=mid-3;
		let max=mid+3;
		let mut total: f64=0.0;
		let mut i=min;
		while i<=max {
			total+=sorted[i];
			i+=1;
		}
		total/((max-min)+1) as f64
    }

	// pub fn calculate_average(&self) -> f64 {
	// 	if self.data.is_empty() {
    //         return 0.0;
    //     }
    //     let sum: f64 = self.data.iter().sum();
    //     sum / self.data.len() as f64
	// }

	pub fn calculate_max(&self) -> f64 {
        self.data.iter().cloned().max_by(|a, b| {
            if a.is_nan() {
                std::cmp::Ordering::Greater
            } else if b.is_nan() {
                std::cmp::Ordering::Less
            } else {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            }
        }).unwrap_or(0.0)
    }

    pub fn calculate_min(&self) -> f64 {
        self.data.iter().cloned().min_by(|a, b| {
            if a.is_nan() {
                std::cmp::Ordering::Greater
            } else if b.is_nan() {
                std::cmp::Ordering::Less
            } else {
                a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
            }
        }).unwrap_or(0.0)
    }
}


