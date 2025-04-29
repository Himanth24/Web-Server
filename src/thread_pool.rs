use std::{
    sync::{mpsc, Arc, Mutex},
    panic::{ catch_unwind, AssertUnwindSafe},
    thread,
};

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: Option<mpsc::Sender<Message>>,
}

enum Message {
    NewJob(Job),
    Terminate,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel::<Message>();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id, Arc::clone(&receiver)));
        }

        ThreadPool { 
            workers, 
            sender: Some(sender),
        }
    }

    pub fn execute<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let job = Box::new(f);
        
        if let Some(sender) = &self.sender {
            sender.send(Message::NewJob(job)).unwrap_or_else(|e| {
                eprintln!("Error sending job to worker: {}", e);
            });
        } else {
            eprintln!("Cannot execute job: thread pool is shutting down");
        }
    }

    pub fn shutdown(&mut self) {
        println!("Shutting down thread pool...");
        
        // Take ownership of sender, which will cause it to be dropped when this function ends
        if let Some(sender) = self.sender.take() {
            // Send terminate message to all workers
            for _ in &self.workers {
                sender.send(Message::Terminate).unwrap_or_else(|_| {
                    eprintln!("Error sending terminate message to worker");
                });
            }
            
            // Drop the sender to close the channel
            drop(sender);
            
            // Join all worker threads
            for worker in &mut self.workers {
                println!("Shutting down worker {}", worker.id);
                if let Some(thread) = worker.thread.take() {
                    thread.join().unwrap_or_else(|e| {
                        eprintln!("Error joining worker thread {}: {:?}", worker.id, e);
                    });
                }
            }
            
            println!("Thread pool shut down complete");
        }
    }
}

// Implement Drop to ensure proper cleanup when ThreadPool is dropped
impl Drop for ThreadPool {
    fn drop(&mut self) {
        if self.sender.is_some() {
            self.shutdown();
        }
    }
}

struct Worker {
    id: usize,
    thread: Option<thread::JoinHandle<()>>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Message>>>) -> Self {
        let thread = thread::spawn(move || {
            loop {
                // Use a block to ensure the lock is released after receiving a message
                let message = {
                    let receiver_lock = receiver.lock().unwrap_or_else(|e| {
                        eprintln!("Worker {} failed to lock receiver: {:?}", id, e);
                        e.into_inner()
                    });
                    
                    match receiver_lock.recv() {
                        Ok(message) => message,
                        Err(e) => {
                            eprintln!("Worker {} disconnected: {}", id, e);
                            break;
                        }
                    }
                };

                match message {
                    Message::NewJob(job) => {
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            job();
                        }));
                        
                        if let Err(e) = result {
                            eprintln!("Worker {} panicked while executing job: {:?}", id, e);
                        }
                    }
                    Message::Terminate => {
                        println!("Worker {} received terminate message", id);
                        break;
                    }
                }
            }
            
            println!("Worker {} shutting down", id);
        });

        Worker { 
            id, 
            thread: Some(thread),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_single_job_execution() {
        let pool = ThreadPool::new(2);
        let flag = Arc::new(Mutex::new(false));

        let flag_clone = Arc::clone(&flag);
        pool.execute(move || {
            let mut flag = flag_clone.lock().unwrap();
            *flag = true;
        });

        // Wait a bit to make sure thread runs
        thread::sleep(Duration::from_millis(50));

        assert_eq!(*flag.lock().unwrap(), true);
    }

    #[test]
    fn test_multiple_jobs() {
        let pool = ThreadPool::new(4);
        let results = Arc::new(Mutex::new(Vec::new()));

        for i in 0..5 {
            let results_clone = Arc::clone(&results);
            pool.execute(move || {
                let mut vec = results_clone.lock().unwrap();
                vec.push(i);
            });
        }

        thread::sleep(Duration::from_millis(100));

        let results = results.lock().unwrap();
        assert_eq!(results.len(), 5);
        for i in 0..5 {
            assert!(results.contains(&i));
        }
    }

    #[test]
    fn test_jobs_run_concurrently() {
        let pool = ThreadPool::new(2);
        let start_time = std::time::Instant::now();

        for _ in 0..2 {
            pool.execute(|| {
                thread::sleep(Duration::from_millis(100));
            });
        }

        // If run sequentially, this would take ~200ms. We check it finishes sooner.
        thread::sleep(Duration::from_millis(150));
        let elapsed = start_time.elapsed();
        assert!(elapsed < Duration::from_millis(160));
    }

    #[test]
    fn test_shut_down() {
        let mut pool = ThreadPool::new(2);
        for _ in 0..2 {
            pool.execute (|| {
                thread::sleep(Duration::from_millis(100));
            });
        }
        pool.shutdown();
    }
}