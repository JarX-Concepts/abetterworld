#include "draco/core/decoder_buffer.h"
#include "draco/mesh/mesh.h"
#include "draco/compression/decode.h"
#include <vector>
#include <cstring>

extern "C" {

struct Vertex {
    float position[3];
    float normal[3];
    float color[4];
    float texcoord0[2];
    float texcoord1[2];
};

struct DecodedMesh {
    Vertex* vertices;
    int vertex_count;

    uint32_t* indices;
    int index_count;
};

bool decode_draco_mesh_interleaved(const uint8_t* data, size_t length, DecodedMesh* out) {
    draco::DecoderBuffer buffer;
    buffer.Init(reinterpret_cast<const char*>(data), length);

    draco::Decoder decoder;
    auto result = decoder.DecodeMeshFromBuffer(&buffer);
    if (!result.ok()) {
        return false;
    }

    std::unique_ptr<draco::Mesh> mesh = std::move(result).value();
    int num_points = mesh->num_points();

    // Attributes
    const draco::PointAttribute* pos_attr = mesh->GetNamedAttribute(draco::GeometryAttribute::POSITION);
    if (!pos_attr) return false;

    const draco::PointAttribute* normal_attr = mesh->GetNamedAttribute(draco::GeometryAttribute::NORMAL);
    const draco::PointAttribute* color_attr = mesh->GetNamedAttribute(draco::GeometryAttribute::COLOR);

    // Find up to 2 texcoord sets
    const draco::PointAttribute* texcoord_attrs[2] = {nullptr, nullptr};
    int texcoord_found = 0;
    for (int i = 0; i < mesh->num_attributes() && texcoord_found < 2; ++i) {
        const draco::PointAttribute* attr = mesh->attribute(i);
        if (attr->attribute_type() == draco::GeometryAttribute::TEX_COORD) {
            texcoord_attrs[texcoord_found++] = attr;
        }
    }

    // Allocate output
    out->vertex_count = num_points;
    out->vertices = new Vertex[num_points];

    for (draco::PointIndex i(0); i < mesh->num_points(); ++i) {
        Vertex v = {};

        // Position (always present)
        pos_attr->GetValue(pos_attr->mapped_index(i), v.position);

        // Normal
        if (normal_attr) {
            normal_attr->GetValue(normal_attr->mapped_index(i), v.normal);
        } else {
            v.normal[0] = 0.0f; v.normal[1] = 0.0f; v.normal[2] = 1.0f;
        }

        // Color
        if (color_attr) {
            float color[4] = {1.0f, 1.0f, 1.0f, 1.0f};
            color_attr->GetValue(color_attr->mapped_index(i), color);
            std::memcpy(v.color, color, sizeof(float) * color_attr->num_components());
            if (color_attr->num_components() < 4) {
                v.color[3] = 1.0f; // opaque default
            }
        } else {
            v.color[0] = v.color[1] = v.color[2] = v.color[3] = 1.0f;
        }

        // Texcoord sets
        for (int tc = 0; tc < 2; ++tc) {
            if (texcoord_attrs[tc]) {
                float uv[2] = {0.0f, 0.0f};
                texcoord_attrs[tc]->GetValue(texcoord_attrs[tc]->mapped_index(i), uv);
                v.texcoord0[0] = (tc == 0) ? uv[0] : v.texcoord0[0];
                v.texcoord0[1] = (tc == 0) ? uv[1] : v.texcoord0[1];
                v.texcoord1[0] = (tc == 1) ? uv[0] : v.texcoord1[0];
                v.texcoord1[1] = (tc == 1) ? uv[1] : v.texcoord1[1];
            } else {
                if (tc == 0) v.texcoord0[0] = v.texcoord0[1] = 0.0f;
                if (tc == 1) v.texcoord1[0] = v.texcoord1[1] = 0.0f;
            }
        }

        out->vertices[i.value()] = v;
    }

    // Indices
    out->index_count = mesh->num_faces() * 3;
    out->indices = new uint32_t[out->index_count];
    for (draco::FaceIndex i(0); i < mesh->num_faces(); ++i) {
        const draco::Mesh::Face& face = mesh->face(i);
        for (int j = 0; j < 3; ++j) {
            out->indices[i.value() * 3 + j] = face[j].value();
        }
    }

    return true;
}

void free_decoded_mesh(DecodedMesh* mesh) {
    delete[] mesh->vertices;
    delete[] mesh->indices;
    mesh->vertices = nullptr;
    mesh->indices = nullptr;
    mesh->vertex_count = 0;
    mesh->index_count = 0;
}

} // extern "C"