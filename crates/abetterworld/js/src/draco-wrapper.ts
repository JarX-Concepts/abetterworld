/// <reference lib="webworker" />
import { DracoRequest, DracoResponse, DracoControl } from "./types";
const inFlight = new Set<number>(); // jobIds for coarse cancellation

const DRACO_JS =
  "https://www.gstatic.com/draco/versioned/decoders/1.5.7/draco_decoder.js";
const DRACO_WASM =
  "https://www.gstatic.com/draco/versioned/decoders/1.5.7/draco_decoder.wasm";

let draco: any | null = null;
let dracoReady: Promise<any> | null = null;

async function ensureDraco(): Promise<any> {
  if (draco) return draco;
  if (!dracoReady) {
    dracoReady = (async () => {
      // 1) load the script into this classic worker’s global scope
      (self as any).importScripts(DRACO_JS);

      // 2) create the module; tell it where to find the .wasm
      const mod = await (self as any).DracoDecoderModule({
        locateFile: (path: string) =>
          path.endsWith(".wasm") ? DRACO_WASM : path,
        onModuleLoaded: () => console.log("[Draco] Decoder ready"),
      });

      return mod;
    })();
  }
  draco = await dracoReady;
  return draco;
}

function respond(msg: DracoResponse, transfers: Transferable[] = []) {
  (self as any).postMessage(msg, transfers);
}

type AttrType = number; // draco.POSITION, NORMAL, COLOR, TEX_COORD

function getNamedAttribute(mesh: any, decoder: any, type: AttrType) {
  for (let i = 0; i < mesh.num_attributes(); ++i) {
    const attr = decoder.GetAttribute(mesh, i);
    if (attr.attribute_type() === type) return attr;
  }
  return null;
}

function tryGetFloatArray(
  draco: any,
  decoder: any,
  mesh: any,
  attr: any
): Float32Array | null {
  if (!attr) return null;
  const arr = new draco.DracoFloat32Array();
  const ok = decoder.GetAttributeFloatForAllPoints(mesh, attr, arr);
  if (!ok) {
    draco.destroy(arr);
    return null;
  }
  const len = mesh.num_points() * attr.num_components();
  const out = new Float32Array(len);
  // Bulk copy in chunks to keep GC friendly
  for (let i = 0; i < len; i++) out[i] = arr.GetValue(i);
  draco.destroy(arr);
  return out;
}

function buildInterleaved(
  numPoints: number,
  pos: Float32Array,
  normal: Float32Array | null,
  color: Float32Array | null,
  colorComps: number,
  uv0: Float32Array | null,
  uv1: Float32Array | null
): Float32Array {
  const VTX_SIZE = 3 + 3 + 4 + 2 + 2;
  const out = new Float32Array(numPoints * VTX_SIZE);

  // Chunk to avoid long stalls (8–32k points per chunk is a good start)
  const CHUNK = 16384;
  for (let base = 0; base < numPoints; base += CHUNK) {
    const end = Math.min(numPoints, base + CHUNK);
    for (let i = base; i < end; i++) {
      let j = 0;
      const off = i * VTX_SIZE;

      // position
      out[off + j++] = pos[i * 3 + 0];
      out[off + j++] = pos[i * 3 + 1];
      out[off + j++] = pos[i * 3 + 2];

      // normal (default 0,0,1)
      if (normal) {
        out[off + j++] = normal[i * 3 + 0];
        out[off + j++] = normal[i * 3 + 1];
        out[off + j++] = normal[i * 3 + 2];
      } else {
        out[off + j++] = 0;
        out[off + j++] = 0;
        out[off + j++] = 1;
      }

      // color (pad to 4)
      if (color) {
        for (let c = 0; c < colorComps; c++) {
          out[off + j++] = color[i * colorComps + c];
        }
        while (j < 3 + 3 + 4) out[off + j++] = 1.0;
      } else {
        out[off + j++] = 1;
        out[off + j++] = 1;
        out[off + j++] = 1;
        out[off + j++] = 1;
      }

      // uv0
      if (uv0) {
        out[off + j++] = uv0[i * 2 + 0];
        out[off + j++] = uv0[i * 2 + 1];
      } else {
        out[off + j++] = 0;
        out[off + j++] = 0;
      }

      // uv1
      if (uv1) {
        out[off + j++] = uv1[i * 2 + 0];
        out[off + j++] = uv1[i * 2 + 1];
      } else {
        out[off + j++] = 0;
        out[off + j++] = 0;
      }
    }
    // Yield back to event loop between chunks
    // (microtask queue drain – keeps main thread smooth even if worker is busy)
  }
  return out;
}

async function handleDecode(req: DracoRequest) {
  const { jobId, arrayBuffer } = req;
  inFlight.add(jobId);
  try {
    const draco = await ensureDraco();
    if (!inFlight.has(jobId)) return; // canceled during load

    const decoder = new draco.Decoder();
    const buffer = new draco.DecoderBuffer();

    // Copy compressed data into WASM heap
    const bufPtr = draco._malloc(arrayBuffer.byteLength);
    draco.HEAPU8.set(new Uint8Array(arrayBuffer), bufPtr);
    buffer.Init(bufPtr, arrayBuffer.byteLength);

    const geomType = decoder.GetEncodedGeometryType(buffer);
    if (geomType !== draco.TRIANGULAR_MESH) {
      throw new Error("Unsupported geometry type (not TRIANGULAR_MESH)");
    }

    const mesh = new draco.Mesh();
    const status = decoder.DecodeBufferToMesh(buffer, mesh);
    if (!status.ok()) {
      throw new Error("Draco decode failed: " + status.error_msg());
    }

    const numPoints = mesh.num_points();
    const numFaces = mesh.num_faces();

    const posAttr = getNamedAttribute(mesh, decoder, draco.POSITION);
    const nrmAttr = getNamedAttribute(mesh, decoder, draco.NORMAL);
    const colAttr = getNamedAttribute(mesh, decoder, draco.COLOR);

    // first two TEX_COORD attributes (if present)
    const tex0 = (() => {
      for (let i = 0; i < mesh.num_attributes(); ++i) {
        const a = decoder.GetAttribute(mesh, i);
        if (a.attribute_type() === draco.TEX_COORD) return a;
      }
      return null;
    })();
    const tex1 = (() => {
      let found = 0;
      for (let i = 0; i < mesh.num_attributes(); ++i) {
        const a = decoder.GetAttribute(mesh, i);
        if (a.attribute_type() === draco.TEX_COORD) {
          if (found === 0) {
            found++;
            continue;
          }
          return a;
        }
      }
      return null;
    })();

    const pos = tryGetFloatArray(draco, decoder, mesh, posAttr);
    if (!pos) throw new Error("Missing/invalid POSITION attribute");

    const nrm = tryGetFloatArray(draco, decoder, mesh, nrmAttr);
    const col = tryGetFloatArray(draco, decoder, mesh, colAttr);
    const colComps = colAttr ? colAttr.num_components() : 0;
    const uv0 = tryGetFloatArray(draco, decoder, mesh, tex0);
    const uv1 = tryGetFloatArray(draco, decoder, mesh, tex1);

    if (!inFlight.has(jobId)) return; // canceled

    // Interleave (chunked to stay responsive on lower-end CPUs)
    const vertices = buildInterleaved(
      numPoints,
      pos,
      nrm,
      col,
      colComps,
      uv0,
      uv1
    );

    // Indices (choose 16-bit when possible)
    const numIndices = numFaces * 3;
    let indices: Uint32Array;

    const byteLen = numIndices * 4;
    const ptr = draco._malloc(byteLen);
    const ok = decoder.GetTrianglesUInt32Array(mesh, byteLen, ptr);
    if (!ok) throw new Error("Failed to decode indices (u32)");
    indices = new Uint32Array(draco.HEAPU32.buffer, ptr, numIndices).slice(); // copy out
    draco._free(ptr);

    // Cleanup Draco objects
    draco._free(bufPtr);
    draco.destroy(buffer);
    draco.destroy(mesh);
    draco.destroy(decoder);

    if (!inFlight.has(jobId)) return; // canceled after compute

    // Transfer back (zero-copy transfer of backing stores)
    respond(
      {
        kind: "ok",
        jobId,
        vertices,
        indices,
        indexCount: numIndices,
        vertexCount: numPoints,
      },
      [vertices.buffer, indices.buffer]
    );
  } catch (e: any) {
    if (inFlight.has(jobId)) {
      respond({ kind: "err", jobId, message: String(e?.message || e) });
    }
  } finally {
    inFlight.delete(jobId);
  }
}

self.onmessage = (ev: MessageEvent<DracoRequest | DracoControl>) => {
  const msg = ev.data;
  if (msg.kind === "decode") {
    // ArrayBuffer is already transferred to the worker at this point.
    handleDecode(msg);
  } else if (msg.kind === "cancel") {
    inFlight.delete(msg.jobId);
  } else if (msg.kind === "init") {
    // No-op; ensureDraco() will lazy-load on first decode
  }
};
