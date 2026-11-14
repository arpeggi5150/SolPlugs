// C ABI over Song Sense's analysis core. This file is SolPlugs' own binding
// (MIT OR Apache-2.0); Song Sense's sources are compiled from a private checkout.
#include <cstddef>
#include <cstring>
#include <string>
#include <vector>

#include "Types.h"

extern "C" {

struct ss_shim_result {
    int has_bpm;
    double bpm;
    double bpm_confidence;
    double key_confidence;
    char key[32];
};

// 0 = analysed, 1 = Song Sense refused the audio, 2 = unexpected failure.
int ss_shim_analyse(const float* mono, size_t frames, double sample_rate, ss_shim_result* out) {
    if (!mono || !out || frames == 0 || !(sample_rate > 0)) return 1;
    std::memset(out, 0, sizeof(*out));
    try {
        std::vector<float> v(mono, mono + frames);
        ss::Analysis a = ss::analyse(v, sample_rate, "sample");
        if (a.overallBpm) {
            out->has_bpm = 1;
            out->bpm = *a.overallBpm;
        }
        // Tempo confidence: the sections' confidences, weighted by length.
        double w = 0, c = 0;
        for (const auto& seg : a.segments) {
            if (!seg.bpm) continue;
            double len = seg.endSec - seg.startSec;
            w += len;
            c += len * seg.bpmConfidence;
        }
        out->bpm_confidence = w > 0 ? c / w : 0;
        out->key_confidence = a.overallKeyConfidence;
        std::strncpy(out->key, a.overallKey.c_str(), sizeof(out->key) - 1);
        return 0;
    } catch (const ss::SongSenseError&) {
        return 1;
    } catch (...) {
        return 2;
    }
}

const char* ss_shim_version() { return ss::kVersion; }

}
