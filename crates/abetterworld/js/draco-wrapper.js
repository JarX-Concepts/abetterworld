let dracoDecoderModule;

export async function initDraco() {
  console.log("[Draco] Initializing decoder module...");
  return new Promise((resolve) => {
    DracoDecoderModule({
      wasmBinaryFile:
        "https://www.gstatic.com/draco/versioned/decoders/1.5.7/draco_decoder.wasm",
      onModuleLoaded: (module) => {
        console.log("[Draco] Decoder module loaded.");
        dracoDecoderModule = module;
        resolve();
      },
    });
  });
}

export function decodeDracoBuffer(buffer) {
  try {
    const decoder = new dracoDecoderModule.Decoder();

    const bufferPtr = dracoDecoderModule._malloc(buffer.byteLength);
    dracoDecoderModule.HEAPU8.set(new Uint8Array(buffer), bufferPtr);

    const dracoBuffer = new dracoDecoderModule.DecoderBuffer();
    dracoBuffer.Init(bufferPtr, buffer.byteLength);

    const geometryType = decoder.GetEncodedGeometryType(dracoBuffer);
    if (geometryType !== dracoDecoderModule.TRIANGULAR_MESH) {
      throw new Error("Unsupported geometry type (not TRIANGULAR_MESH)");
    }

    const mesh = new dracoDecoderModule.Mesh();
    const status = decoder.DecodeBufferToMesh(dracoBuffer, mesh);
    if (!status.ok()) {
      throw new Error("Draco decode failed: " + status.error_msg());
    }

    const numPoints = mesh.num_points();
    const numFaces = mesh.num_faces();
    const vertexSize = 3 + 3 + 4 + 2 + 2;
    const vertices = new Float32Array(numPoints * vertexSize);

    const getNamedAttribute = (mesh, decoder, type) => {
      for (let i = 0; i < mesh.num_attributes(); ++i) {
        const attr = decoder.GetAttribute(mesh, i);
        if (attr.attribute_type() === type) {
          return attr;
        }
      }
      return null;
    };

    const getAttrArray = (attr, label) => {
      if (!attr) return null;
      const numComponents = attr.num_components();
      const array = new dracoDecoderModule.DracoFloat32Array();
      const success = decoder.GetAttributeFloatForAllPoints(mesh, attr, array);
      if (success) {
        const result = new Float32Array(numPoints * numComponents);
        for (let i = 0; i < result.length; ++i) {
          result[i] = array.GetValue(i);
        }
        dracoDecoderModule.destroy(array);
        return result;
      } else {
        dracoDecoderModule.destroy(array);
        console.warn(`[Draco] Fast decode failed for ${label}, using fallback`);
        return null;
      }
    };

    const posAttr = getNamedAttribute(
      mesh,
      decoder,
      dracoDecoderModule.POSITION
    );
    const normalAttr = getNamedAttribute(
      mesh,
      decoder,
      dracoDecoderModule.NORMAL
    );
    const colorAttr = getNamedAttribute(
      mesh,
      decoder,
      dracoDecoderModule.COLOR
    );

    const texcoordAttrs = [];
    for (let i = 0; i < mesh.num_attributes(); ++i) {
      const attr = decoder.GetAttribute(mesh, i);
      if (attr.attribute_type() === dracoDecoderModule.TEX_COORD) {
        texcoordAttrs.push(attr);
      }
    }

    const posData = getAttrArray(posAttr, "position");
    const normalData = getAttrArray(normalAttr, "normal");
    const colorData = getAttrArray(colorAttr, "color");
    const colorComponents = colorAttr?.num_components() || 0;
    const texcoord0Data = getAttrArray(texcoordAttrs[0], "texcoord0");
    const texcoord1Data = getAttrArray(texcoordAttrs[1], "texcoord1");

    for (let i = 0; i < numPoints; ++i) {
      const offset = i * vertexSize;
      let j = 0;

      // position
      vertices[offset + j++] = posData[i * 3 + 0];
      vertices[offset + j++] = posData[i * 3 + 1];
      vertices[offset + j++] = posData[i * 3 + 2];

      // normal
      if (normalData) {
        vertices[offset + j++] = normalData[i * 3 + 0];
        vertices[offset + j++] = normalData[i * 3 + 1];
        vertices[offset + j++] = normalData[i * 3 + 2];
      } else {
        vertices[offset + j++] = 0;
        vertices[offset + j++] = 0;
        vertices[offset + j++] = 1;
      }

      // color
      if (colorData) {
        for (let c = 0; c < colorComponents; ++c) {
          vertices[offset + j++] = colorData[i * colorComponents + c];
        }
        while (j < offset + 10) vertices[offset + j++] = 1.0;
      } else {
        vertices[offset + j++] = 1;
        vertices[offset + j++] = 1;
        vertices[offset + j++] = 1;
        vertices[offset + j++] = 1;
      }

      // texcoord0
      if (texcoord0Data) {
        vertices[offset + j++] = texcoord0Data[i * 2 + 0];
        vertices[offset + j++] = texcoord0Data[i * 2 + 1];
      } else {
        vertices[offset + j++] = 0;
        vertices[offset + j++] = 0;
      }

      // texcoord1
      if (texcoord1Data) {
        vertices[offset + j++] = texcoord1Data[i * 2 + 0];
        vertices[offset + j++] = texcoord1Data[i * 2 + 1];
      } else {
        vertices[offset + j++] = 0;
        vertices[offset + j++] = 0;
      }
    }

    const numIndices = numFaces * 3;
    const indices = new Uint32Array(numIndices);
    const indicesPtr = dracoDecoderModule._malloc(indices.byteLength);

    const success = decoder.GetTrianglesUInt32Array(
      mesh,
      indices.byteLength,
      indicesPtr
    );
    if (!success) throw new Error("Failed to decode triangle indices");

    // Copy the output from WASM memory into JS
    indices.set(
      new Uint32Array(
        dracoDecoderModule.HEAPU32.buffer,
        indicesPtr,
        indices.length
      )
    );

    dracoDecoderModule._free(indicesPtr);

    dracoDecoderModule._free(bufferPtr);
    dracoDecoderModule.destroy(dracoBuffer);
    dracoDecoderModule.destroy(mesh);
    dracoDecoderModule.destroy(decoder);

    return { vertices, indices };
  } catch (e) {
    console.error("[Draco] Exception during decode:", e);
    return null;
  }
}
