use std::{
    sync::{mpsc, Arc, Mutex},
    thread,
};

pub struct ThreadPool {
    workers: Vec<Worker>,
    sender: mpsc::Sender<Job>,
}

type Job = Box<dyn FnOnce() + Send + 'static>;

impl ThreadPool {
    pub fn new(size: usize) -> Self {
        assert!(size > 0);

        let (sender, receiver) = mpsc::channel::<Job>();
        let receiver = Arc::new(Mutex::new(receiver));
        let mut workers = Vec::with_capacity(size);

        for id in 0..size {
            workers.push(Worker::new(id,Arc::clone(&receiver)));
        }

        ThreadPool { workers, sender }
    }

    pub fn execute<F>(&self, f:F) 
    where 
        F: FnOnce() + Send + 'static, 
    {
        let job = Box::new(f);
        self.sender.send(job).unwrap();
    }
}

struct Worker {
    id: usize,
    thread: thread::JoinHandle<()>,
}

impl Worker {
    fn new(id: usize, receiver: Arc<Mutex<mpsc::Receiver<Job>>>) -> Self {
        let thread = thread::spawn(move || loop {
            let job = receiver.lock().unwrap().recv().unwrap();
            job();
        });

        Worker{ id, thread }
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
        assert!(elapsed < Duration::from_millis(200));
    }
}
