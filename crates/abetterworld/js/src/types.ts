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

export type Position = number[];
export type Deg = number;
export type FrustumPlanes = number[][];

export type CameraState = {
  position: Position;
  far: number;
  fovy: Deg;
  planes: FrustumPlanes;
  screen_height: number;
  sse_threshold: number;
};

export type TileKey = number;
export type Gen = number;
export type ChildrenKeys = TileKey[];
export type RefineMode = "ADD" | "REPLACE";
export type BoundingVolume = number[];

export type PagerMessage =
  | {
      kind: "camera_update";
      camera: CameraState;
    }
  | {
      kind: "init";
      wasm_url: string;
    };

export type TileMessage =
  | {
      kind: "Load";
      key: TileKey;
      gen: Gen;

      uri: string;
    }
  | {
      kind: "Unload";
      key: TileKey;
      gen: Gen;
    }
  | {
      kind: "Update";
      key: TileKey;
      gen: Gen;

      children?: ChildrenKeys;
      parent?: TileKey;
      volume: BoundingVolume;
      refine: RefineMode;
      geometric_error: number;
    };
