type Job = { run: () => Promise<void>; signal: AbortSignal; queued: boolean;
  resolve: () => void; reject: (reason: unknown) => void; detach: () => void };

// Only thumbnail work: two active jobs, canceled queued jobs are removed.
export class ThumbnailQueue {
  private jobs: Job[] = [];
  private running = 0;
  get active() { return this.running; }
  get queued() { return this.jobs.length; }
  schedule(run: () => Promise<void>, signal: AbortSignal): Promise<void> {
    if (signal.aborted) return Promise.reject(signal.reason);
    return new Promise((resolve, reject) => {
      const job: Job = { run, signal, queued: true, resolve, reject, detach: () => {} };
      const abort = () => {
        if (job.queued) { this.jobs = this.jobs.filter(value => value !== job); job.detach(); }
        reject(signal.reason);
      };
      job.detach = () => signal.removeEventListener('abort', abort);
      signal.addEventListener('abort', abort, { once: true });
      this.jobs.push(job); this.drain();
    });
  }
  private drain() {
    while (this.running < 2 && this.jobs.length) {
      const job = this.jobs.shift()!; job.queued = false; this.running++;
      Promise.resolve().then(() => { job.signal.throwIfAborted(); return job.run(); })
        .then(job.resolve, job.reject).finally(() => { job.detach(); this.running--; this.drain(); });
    }
  }
}
