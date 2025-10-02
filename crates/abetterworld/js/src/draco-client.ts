import { DracoRequest, DracoResponse, DracoResponseOk } from "./types";

export class DracoWorkerClient {
  private worker: Worker;
  private nextJobId = 1;
  private pending = new Map<
    number,
    {
      resolve: (v: Partial<DracoResponseOk>) => void;
      reject: (e: any) => void;
    }
  >();

  constructor(workerUrl: string) {
    this.worker = new Worker(workerUrl, { type: "classic" });
    this.worker.onmessage = (ev: MessageEvent<DracoResponse>) => {
      const msg = ev.data;
      const slot = this.pending.get(msg.jobId);
      if (!slot) return;
      this.pending.delete(msg.jobId);
      if (msg.kind === "ok") {
        slot.resolve({
          ...msg,
        });
      } else {
        slot.reject(new Error(msg.message));
      }
    };
  }

  decode(buffer: ArrayBuffer): {
    jobId: number;
    promise: Promise<Partial<DracoResponseOk>>;
    cancel: () => void;
  } {
    const jobId = this.nextJobId++;
    const promise = new Promise<Partial<DracoResponseOk>>((resolve, reject) => {
      this.pending.set(jobId, { resolve, reject });
      const msg: DracoRequest = {
        kind: "decode",
        jobId,
        arrayBuffer: buffer,
      };
      // Transfer the buffer so we don't copy it
      this.worker.postMessage(msg, [buffer]);
    });

    const cancel = () => {
      if (this.pending.delete(jobId)) {
        this.worker.postMessage({ kind: "cancel", jobId });
      }
    };

    return { jobId, promise, cancel };
  }

  dispose() {
    this.worker.terminate();
    this.pending.forEach(({ reject }, id) => {
      reject(new Error("Worker terminated"));
      this.pending.delete(id);
    });
  }
}
