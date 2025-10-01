export type DracoRequest = {
  kind: "decode";
  jobId: number;
  arrayBuffer: ArrayBuffer; // TRANSFERRED to worker
};

export type DracoResponseOk = {
  kind: "ok";
  jobId: number;
  vertices: Float32Array; // TRANSFERRED back
  indices: Uint32Array; // TRANSFERRED back
  indexCount: number;
  vertexCount: number;
};

export type DracoResponseErr = {
  kind: "err";
  jobId: number;
  message: string;
};

export type DracoResponse = DracoResponseOk | DracoResponseErr;

export type DracoControl = { kind: "init" } | { kind: "cancel"; jobId: number };
