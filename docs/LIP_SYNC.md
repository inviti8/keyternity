# Shapes, dialog and auto lip sync

Status: **research and scope.** Nothing is built. Decisions to confirm are in §11.

## 1. Goal

Get a talking character from a line of dialog in minutes, then animate expression on
top of it by hand. It works the way Facebot did in MotionBuilder:
1. An audio file goes to a phoneme analyser, which gives timed, weighted phonemes.
2. The phonemes map onto a fixed set of Preston Blair visemes.
3. The visemes become keys on the timeline, driving a face that follows a known
   convention.
4. The artist then keys brows, eyes and smiles over it.

For Keyternity this breaks down into building blocks, each useful on its own:

0. **Animation layers:** keys in stacked layers (override or additive) that combine,
   so generated lip sync and hand animation never fight over the same keys (§3).
1. **Shapes:** morph targets for 2D meshes, with keyable weights. They're useful for
   any facial or squash-and-stretch animation, not just speech.
2. **Dialog audio** in animations: import a clip, see its waveform, hear it in sync.
3. **Mouth rigs and visemes:** a standard viseme set, and a per-character mapping
   from each viseme to shapes or a mouth texture.
4. **Auto lip sync:** audio (and optional text) gives a viseme track, which gives
   keys. Regenerating it doesn't touch the artist's own keys.
5. **Voice generation:** type a line, pick a voice, get the audio and its timing.
6. **Agent tools,** so the MCP agent can do all of the above.

## 2. How 2D lip sync is done, and where Keyternity stands

| approach | how | Keyternity today |
|---|---|---|
| **Mouth swaps** (Moho switch layers, Spine attachments, the classic "mouth chart") | One drawing per viseme, swapped on the beat | **Possible now:** Texture keys swap a bone's texture. No lip sync drives them yet. |
| **Mesh morphs / blend shapes** (Live2D parameters, Spine mesh deform keys, 3D blend shapes) | Sculpted vertex offsets per shape, blended by weight. Shapes add up: "O" + "smile". | **Missing:** meshes have no per-vertex animation. Topology, binds and weights exist. |
| **Bone-driven mouths** | Jaw and lip-corner bones posed per viseme | Possible with keys, but there's no pose library. |

Facebot's power came from the morph approach: visemes and expressions are separate
shapes that add together. That's why lip sync can be generated and then left alone
while the artist animates on top. So Shapes come first (as you suggested). Mouth swaps
are supported too, because many 2D artists draw mouths, and they work with the
existing Texture keys.

**Delivering it to games:**
- Frame sequences and spritesheets render shapes as-is.
- DragonBones has per-vertex deform (FFD) timelines for meshes, so shape weights can
  be baked to deform keys on export.
- Native `.skf` runtimes would need the new data (see §4.5).

## 3. Animation layers

Lip sync only works if the generated keys and the artist's keys live in separate places
that combine. Then regenerating lip sync never disturbs the hand animation, and a
smile, blink or head tilt animates over the talking mouth. MotionBuilder's animation
layers did this for Facebot. Keyternity has no layers yet:
- An `Animation` is one flat list of keys per (bone, property). Absolute values are
  interpolated over the setup pose (`Armature::interpolate_keyframes`).
- One animation plays at a time.

So layers come first, before shapes.

### 3.1 Model

Layers live **inside an animation** (like MotionBuilder's layers within a take, and
Blender's NLA tracks on one action):

```rust
pub struct AnimLayer {
    pub id: i32,
    pub name: String,          // "Base", "Lip Sync", "Face", "Breathing"
    pub mode: LayerMode,       // Override | Additive
    pub weight: f32,           // 0..1; keyable later (§3.6)
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,          // no editing (e.g. a generated layer)
    pub generated: Option<i32>,// owned by a lip-sync track (§7.1): regenerate replaces its keys
}
// Animation gains `layers: Vec<AnimLayer>` (bottom to top); Keyframe gains `layer: i32`.
```

- **Compatibility:** layer 0 is **Base**. Every existing key belongs to it
  (`Keyframe.layer` defaults to 0), so current files and code paths are unchanged.
- **Channels:** a layer only affects the channels (bone, property, shape) it has keys
  for. Everything else passes through from below, so layers need no bone masks to
  start with.

### 3.2 Evaluation

For each channel, bottom to top:

```
v = setup value, then the Base layer's keys as today
for each upper layer L with keys on this channel (not muted; only soloed ones if any solo):
    k = L's interpolated value
    Override:  v = lerp(v, k, L.weight)
    Additive:  v = v + L.weight * (k - rest)          // rest = the channel's setup value
```

What "additive" means depends on the property:

| property | additive | override |
|---|---|---|
| position, rotation | adds the offset from the setup pose | blends to the layer's value |
| scale | multiplies by (layer ÷ setup), weighted | blends |
| shape weights (§4) | **adds the weight** (0 = no change), clamped per exclusive group (§6.3) | blends |
| tint | blends toward the layer's colour | blends |
| texture, hidden, z-index, IK flags (discrete) | treated as override | the top layer with a key wins, if its weight ≥ 0.5 |

This runs in `Armature::animate`, before IK and physics, which see the combined pose
just as they see today's. Evaluating several layers costs only a little more than
today, because channels without keys on a layer are skipped quickly.

### 3.3 Editing

The timeline gets a **layer stack** beside the rows:
- Each layer has its name, mode, a weight slider, and mute, solo and lock toggles.
- One layer is **active**. New keys (from transforms, the bone panel, key buttons,
  paste) go there.
- The rows show the active layer's keys. Other layers' keys show as faint ticks, so
  you can see what's happening above and below.
- Actions: *Add Layer*, *Duplicate*, *Merge Down* (bake a layer into the one below),
  *Bake All* (flatten), *Delete*. All are undoable.
- **Generated layers** (lip sync) are locked. Unlocking one, or editing its keys,
  first asks whether to detach it from regeneration.

### 3.4 Lip sync on layers

- The lip-sync track writes into its own **Lip Sync** layer:
  - additive for morph mouths (so expression shapes on other layers add to it);
  - override for drawn mouths (textures).
- *Regenerate* replaces only that layer's keys. *Bake* detaches it into an ordinary
  layer.
- The artist animates the face and body on other layers. Lowering the Lip Sync
  layer's weight mumbles; raising it over 1 isn't allowed.

### 3.5 Saving and export

| target | layers |
|---|---|
| `.skf` (editor file) | Kept as they are: `layers` on animations and `layer` on keys, documented in `skf_readme.md`. Runtimes that predate layers would mix all keys, so `.skfe` exports default to **Flatten layers**. |
| DragonBones | Flattened. Each channel is sampled at the union of its key frames across layers (plus a set rate where curves combine), so the result matches the editor. |
| Frame sequence, spritesheet, video | Rendered from the combined pose as they are. |

Flattening is a pure function (`layers::flatten(animation, armature) -> Animation`),
tested against the evaluator frame by frame.

### 3.6 Later

- **Keyable layer weight:** fade a layer in and out, e.g. a sneer layer at 0 → 1.
- **Bone masks:** limit an override layer to an arm, for "wave while walking".
- **Blending whole animations:** play idle + talk at runtime. That's a runtime
  feature, separate from editor layers.

## 4. Shapes (morph targets)

### 4.1 Data

A mesh bone gets:

```rust
pub struct MeshShape {
    pub id: i32,
    pub name: String,            // "AI", "smile", "blink_L"
    pub deltas: Vec<Vec2>,       // per vertex, in the mesh's own (rest / bind) space
    pub weight: f32,             // setup-pose weight, usually 0
    pub group: String,           // optional: "mouth", "brows"; for the UI and for exclusivity (§6.3)
}
```

- **Saving:** shapes are stored by vertex **index**, like `locked_verts`, because
  vertex ids are re-numbered on save.
- **Mesh edits keep them valid:**
  - Pen/Eraser and Retriangulate remap deltas: new vertices get the delta
    interpolated from their neighbours, and removed ones are dropped.
  - Trace and Reset replace the mesh, so they ask first, as they do for binds.

### 4.2 Evaluation

```
v = rest + Σ weightᵢ · deltaᵢ      (in the mesh's own space)
then binds / envelope skinning / bind pose as today
```

Shapes apply **before** skinning, as blend shapes do in 3D. That way a viseme sculpted
at rest still follows the head bone when it turns. Bind-posed meshes take the deltas
in bind space.

### 4.3 Animating weights

- A new `AnimElement::ShapeWeight`. A keyframe carries which shape in a new
  `Keyframe.shape` field (serde default -1). It's interpolated per (bone, shape) with
  the existing curves and easing.
- The timeline shows one row per shape under its mesh.
- Weights are normally 0–1, but over-driving (up to about 1.5) is allowed for
  exaggeration.

### 4.4 Editing

- A **Shapes** section in the mesh panel lists each shape with a weight slider and a
  key button. It has *Add Shape* (an empty one, or "from the current pose"),
  Duplicate, Mirror (L↔R across the mesh's local X) and Delete.
- **Sculpt mode** (an Edit button on a shape): the existing vertex move, rotate and
  scale and the soft-select brush edit that shape's deltas. All other shapes are
  shown at 0 while it's being sculpted. A "Show others" toggle sculpts on top of the
  current mix.
- **Shape from pose:** pose the bones (e.g. jaw down), then *Capture as shape*. It
  takes the current skinned vertex positions back into rest-space deltas, so a
  bone-built mouth can become a reusable shape.

### 4.5 Export

| target | shapes |
|---|---|
| `.skf` | New fields (`shapes` on the mesh, `ShapeWeight` keys). The runtime spec (`skf_readme.md`) gets a section. SkelForm's runtimes ignore them, so meshes stay at rest there. |
| DragonBones | Each key of any shape weight becomes a `ffd`/deform key: offsets = Σ w·delta, rotated and flipped into the mesh's DB space. Between keys, the weights' curves are sampled at a set rate, because DB tweens offsets linearly. |
| Frame sequence, spritesheet, video | Rendered directly. |

## 5. Dialog audio in animations

- **Clips:** `AudioClip { id, name, file, start_frame, gain, muted }` on an
  animation. The audio file is **embedded** in the `.skf` zip (`audio/<name>.wav`),
  so rigs stay portable.
- **Decoding:** Symphonia (pure Rust; WAV, MP3, OGG, FLAC). rodio has used it as its
  default decoder since 0.21. It's MPL-2.0, which is compatible with Keyternity's
  GPL-3.0.
- **Playback:** rodio/cpal on desktop. On the web, Web Audio through wasm-bindgen,
  later.
- **Timeline:** the clip's waveform is drawn under the timeline rows. Playback stays
  in sync with the animation clock. Scrubbing plays a short slice (audio scrub), which
  is essential for checking sync by ear.
- **Video export** muxes the clip into the MP4. FFmpeg is already there.
- **Exports:** the manifest/Phaser/Godot files can reference the clip with its start
  time, so a game can play the line in sync.

## 6. Visemes and mouth rigs

### 6.1 Viseme sets

**Internally**, lip sync speaks **Oculus's 15 visemes**, the set TalkingHead's text
rules produce (§7.3). It's also the standard for ARKit-style and VRM avatars.
Keyternity adds two to it:
- **L**, which Oculus folds into `nn`;
- **W**, which Oculus folds into `U`.

The ported rules emit those two separately, because Preston Blair draws them
differently. That gives **17 sounds**.

Each **mouth rig chooses the set it draws**, and the 17 sounds map onto it.
**Preston Blair (10)** is the default, as in Facebot:

| sound (internal) | Preston Blair | Simple 5 |
|---|---|---|
| `sil` | rest | closed |
| `PP` (p, b, m) | MBP | closed |
| `FF` (f, v) | FV | mid |
| `TH` (th) | etc | mid |
| `DD` (t, d) | etc | mid |
| `kk` (k, g) | etc | mid |
| `CH` (ch, j, sh) | etc | mid |
| `SS` (s, z) | etc | mid |
| `nn` (n, ng) | etc | mid |
| **`L`** (l) | **L** | mid |
| `RR` (r) | etc | mid |
| `aa` (a, ah) | AI | open |
| `E` (e, eh) | E | wide |
| `I` (i, ee) | E | wide |
| `O` (o, aw) | O | round |
| `U` (oo, u) | U | round |
| **`W`** (w, wh) | **WQ** | round |

The sets offered are:
- **Preston Blair 10** (default);
- **Oculus 15**, for mouths drawn or sculpted to that standard (`L`→`nn`, `W`→`U`);
- **Simple 5** (closed, mid, open, wide, round), for simple or chibi styles.

A rig can also override any row, e.g. send `TH` to L if that character's "th" shows
the tongue.

### 6.2 Mouth rig

A per-character mapping from each viseme to a **mouth pose**:

```rust
pub struct MouthRig {
    pub id: i32,
    pub name: String,               // "Alice"
    pub set: VisemeSet,             // PrestonBlair10 (default), Oculus15, Simple5
    pub overrides: Vec<(String, String)>, // (sound, viseme): per-character tweaks to §6.1
    pub poses: Vec<MouthPose>,      // one per viseme in the set
}
pub struct MouthPose {
    pub viseme: String,
    pub shapes: Vec<(i32, i32, f32)>,        // (mesh bone id, shape id, weight): morph mouths
    pub texture: Option<(i32, String)>,      // (bone id, texture name): drawn mouths
}
```

- A pose can mix both kinds: a jaw shape plus a teeth texture.
- A **Mouth Rig panel** has a row per viseme with a mouth preview. *Auto-fill* matches
  shapes or textures by name (a shape called "MBP", or Oculus's "viseme_PP", fills
  MBP), which is what made Facebot's naming convention work. *Test* plays a phrase through the rig.
- A rig file can hold several mouth rigs, one per character.

### 6.3 Combining with expressions

- **Morph mouths:** viseme shapes and expression shapes (smile, frown, sneer) add up.
  The lip sync keys stay untouched on their own layer (§3.4) while the artist keys
  the expression on another.
- **Exclusive groups:** shapes in the same group as the mouth rig's visemes are
  normalised, so a viseme crossfade never stacks two visemes past full. Expression
  shapes in other groups stay additive.
- **Drawn mouths** can't add up. For those, an expression swaps the whole mouth set,
  e.g. a "smiling" variant of each viseme texture via a Style. Styles are already
  hot-swappable.

## 7. Auto lip sync

### 7.1 Pipeline

```
text + word timings  ← from the voice generator (§8), or from transcribing imported audio (§7.4)
   → text rules (per language): each word → its sounds, with relative lengths (§7.3)
   → sounds stretched over each word's time span → cues (time, sound)
   → mouth rig's viseme set (§6.1) → viseme track
   → keyer: anticipation, holds, smoothing, intensity
   → keys on the mouth rig's shapes or textures, in the Lip Sync layer
```

The transcript stays editable. Fixing a misheard word and pressing *Regenerate* is
the main way an artist corrects lip sync.

**Lip-sync tracks** (`LipSyncTrack { clip, mouth rig, words: Vec<(word, start, end)>, language, settings }`) are
stored with the animation:
- The keys they produce go in the track's own generated **Lip Sync layer** (§3.4).
  *Regenerate* (after changing the audio, text or settings) replaces only that
  layer's keys.
- *Bake* turns it into an ordinary layer the artist owns.
- The artist's other layers (brows, blinks, smiles, head turns) are never touched.

### 7.2 Keyer settings

| setting | default | why |
|---|---|---|
| Lead | 1 frame | Mouths read better slightly ahead of the sound. |
| Minimum hold | 2 frames (24 fps) | Prevents flicker on fast speech. |
| Smoothing | Medium | Shapes only: weight attack and release, and blending between neighbouring visemes (co-articulation). |
| Intensity | 100% | Mumbled to over-enunciated. |
| Rest between phrases | On, after 0.2 s of silence | |
| Key every frame vs only on changes | On changes | Fewer keys, easier to edit. |

Drawn mouths get stepped keys, while shapes get eased weight curves.

### 7.3 Engine: text rules + word timings (TalkingHead's technique)

The engine is the approach from [TalkingHead](https://github.com/met4citizen/TalkingHead)
(MIT), ported to Rust:
- A small **language module** turns each word into its sounds, each with a relative
  start and length. English uses spelling rules; TalkingHead reports about 80%
  accuracy against a pronunciation dictionary, and more on common words. Languages
  with consistent spelling, like Finnish, map letters straight to sounds.
- Those sounds are then **stretched over the word's time span**.
- TalkingHead has modules for English, German, French, Brazilian Portuguese, Finnish
  and Lithuanian. We port them with attribution, which MIT allows under our GPL-3.0.
- The port adds the `L` and `W` sounds (§6.1).
- Its philosophy is simplicity: *"It is often better to be small and fast than to
  aim for 100% accuracy."*

Why this rather than audio recognition:
- **It's plain code:** no external program, model or download for the core.
- **It works the same everywhere,** the web build included.
- **Mistakes are fixable:** they come from the words, so correcting the transcript
  corrects the mouth.

**Not chosen:**
- **Rhubarb Lip Sync** (MIT; audio in, timed shapes A–H/X out, 0.01 s, no weights).
  It's good for recordings with no script, but it's another external binary, and it's
  English-only in practice.
- **A wav2vec2 phoneme recogniser** in ONNX. It gives weighted, multilingual results,
  but needs a large model.

Either could become an optional engine later if transcripts prove too coarse.

**Loudness** stays as a no-download fallback: mouth openness from the audio envelope,
for creatures, robots, gibberish, and lines with no words.

### 7.4 Transcribing imported audio

For a recorded line, **Transcribe** runs **whisper.cpp** locally through `whisper-rs`.
It gives the text and word timestamps, which feed the same engine.
- **The model is a one-time download,** pinned and SHA-checked like FFmpeg: `tiny`
  ~75 MB, `base` ~142 MB, with multilingual and English-only variants.
- **The transcript shows in the Dialog panel** for checking. If the script is already
  known, the artist can paste it, and Whisper is only used to time it.
- **Timing precision:** Whisper's word times can be off by tens of milliseconds. The
  keyer's lead and holds absorb most of that. whisper.cpp's experimental token-level
  timestamps (DTW) tighten it, and if needed a forced-alignment pass can come later.
- **Desktop only.** On the web, the artist types or pastes the transcript, and the
  words are spread evenly over speech found by loudness.

## 8. Voice generation (dialog from text)

A **provider** interface:

```rust
fn synthesize(&self, text: &str, voice: &Voice, opts: &VoiceOpts)
    -> Result<(Wav, Option<Vec<WordTiming>>), String>
```

The text is known, so lip sync only needs **word timings**:
- Providers that give them (Kokoro's durations summed per word; ElevenLabs'
  character timestamps) feed the engine directly.
- For the others, Whisper times the known text (§7.4).

| provider | where | voices | timings | licence and notes |
|---|---|---|---|---|
| **Kokoro-82M** (first) | Local ONNX (`ort`), ~80–330 MB model download | 54 voices, 8 languages (v1.0) | Phonemes from its misaki G2P. Durations come from ONNX exports that output the duration tensor (the Rust Kokoros project reads them). | Apache-2.0 weights. Small and fast, CPU-friendly. No voice cloning. |
| **Chatterbox** (Resemble AI) | Local Python/PyTorch, as a helper process | **Voice cloning** from a reference clip. Emotion "exaggeration" control (original model). 23+ languages (Multilingual V3). | None, so Whisper times the known text | MIT. Every output carries an imperceptible Perth watermark. Heavier: 110M–500M parameters, and a GPU is best. |
| **ElevenLabs** (cloud, optional) | HTTPS with the artist's own API key | Large library and cloning | `/with-timestamps` returns per-character timing | Commercial service. Sends the text off the machine, so it's opt-in, with the key stored like the agent token. |
| Piper | — | — | — | Skipped for now: the original MIT repository was archived in October 2025, and development continued under GPL. Voice licences vary. |

**UI:** a **Dialog** panel on the animation:
1. Type the line and pick a provider and voice (and speed or emotion where supported).
2. *Generate* adds the clip at the playhead and runs lip sync.
3. Takes are kept, so you can audition and swap them.

**Consent:** voice cloning needs the voice owner's consent. The UI says so when a
reference clip is added, and Keyternity ships no celebrity or real-person voices.

## 9. Agent (MCP) tools

These sit alongside the skinning tools:
- `list_shapes`, `set_shape_weight`, `key_shapes`;
- `list_mouth_rigs`, `auto_fill_mouth_rig`;
- `add_dialog` (text, voice) and `import_audio`;
- `lip_sync` (animation, clip, transcript, settings).

The review page (`review_page`) can show a lip-synced animation as a frame strip with
the transcript. That lets an agent block out a scene's dialog, which the artist then
reviews and refines.

## 10. Phases

| phase | contents | notes |
|---|---|---|
| **0. Animation layers** | Layer data (Base = layer 0, so old files load unchanged), evaluation (override and additive per property), the timeline layer stack (active layer, mute, solo, lock, weight), Merge Down, Bake, flattening for `.skfe` and DragonBones, tests against the evaluator | Useful on its own: breathing or a blink cycle over any animation |
| **1. Shapes** | Data, evaluation before skinning, ShapeWeight keys and timeline rows, Shapes panel, sculpt mode, mirror, capture from pose, remapping on topology edits, `.skf` and DragonBones (FFD) export, tests | Useful immediately: blinks, brows, squash. |
| **2. Dialog audio** | Clips embedded in `.skf`, Symphonia decoding, rodio playback in sync, waveform, audio scrub, video export with audio | Desktop first |
| **3. Mouth rigs + lip sync** | The 17 sounds and the viseme-set mappings, mouth rig panel and auto-fill, the English text rules (ported from TalkingHead), typed transcripts with words spread over speech, lip-sync tracks and keyer, regenerate and bake, loudness fallback, drawn and morph mouths | The Facebot loop, end to end. No downloads. |
| **3b. Transcription** | whisper.cpp through `whisper-rs`, model download, word timings, editable transcript; TalkingHead's other language modules | Imported recordings |
| **4. Voice generation** | Provider interface, Kokoro local with timings, Dialog panel with takes; then ElevenLabs (opt-in) and Chatterbox (helper process) | |
| **5. Agent** | MCP tools; optional audio-recognition engines (Rhubarb, ONNX phonemes) only if transcripts prove too coarse | |

## 11. Decisions to confirm

| # | question | recommendation |
|---|---|---|
| Q0 | Layers inside an animation (MotionBuilder-style), with Override and Additive modes, before Shapes? | Yes. Bone masks and keyable weight come later (§3.6). |
| Q1 | Shapes after layers, with drawn-mouth (texture) lip sync also supported in phase 3? | Yes |
| Q2 | Viseme sets? | **Decided:** internally Oculus 15 + L + W (TalkingHead's output). Mouth rigs draw Preston Blair 10 (default), Oculus 15 or Simple 5 through mapping tables (§6.1). |
| Q3 | Audio embedded in the `.skf`, or linked by path? | Embedded (portable); a "linked" option later if files get large |
| Q4 | Voice generation: local only, or cloud allowed too? Voice cloning? | Local Kokoro first; cloud and cloning as opt-in later |
| Q5 | Languages that matter to you beyond English? | — |
| Q6 | Engine? | **Decided:** TalkingHead's technique (text rules + word timings), with Whisper transcription for imported audio. Rhubarb isn't used. Lip sync itself also works on the web; only transcription is desktop-only. |
| Q7 | Does Facebot's phoneme→viseme table, or its rig convention (shape names), exist anywhere we should match? | — |

## Sources

- TalkingHead (the lip-sync technique): [repository](https://github.com/met4citizen/TalkingHead)

- Rhubarb Lip Sync: [repository and README](https://github.com/DanielSWolf/rhubarb-lip-sync), [licence](https://github.com/DanielSWolf/rhubarb-lip-sync/blob/master/LICENSE.md), [releases](https://github.com/DanielSWolf/rhubarb-lip-sync/releases)
- Kokoro-82M: [model card](https://huggingface.co/hexgrad/Kokoro-82M); durations in the Rust port: [Kokoros](https://docsearch.algolia.com/mcp/docs/repo/lucasjinreal/kokoros)
- Chatterbox: [repository](https://github.com/resemble-ai/chatterbox)
- ElevenLabs timestamps: [API reference](https://elevenlabs.io/docs/api-reference/streaming-with-timestamps), [announcement](https://elevenlabs.io/blog/new-text-to-speech-endpoints-with-timestamps)
- Phoneme recognition in ONNX: [wav2vec2-espeak-ctc export](https://huggingface.co/sadda-speech/wav2vec2-espeak-ctc), [CTC aligner](https://huggingface.co/omote-ai/ctc-aligner), [torchaudio forced alignment](https://pytorch.org/audio/main/tutorials/forced_alignment_tutorial.html)
- Piper status: [overview](https://www.cekura.ai/discover/piper-tts)
- Audio in Rust: [rodio](https://docs.rs/crate/rodio/0.20.0), [Symphonia](https://www.mintlify.com/pdeljanov/Symphonia/resources/faq)
- whisper.cpp in Rust (transcription, word timings): [whisper-rs](https://docs.rs/crate/whisper-rs/latest)
- Rhubarb's output formats (considered, not chosen): [README](https://github.com/DanielSWolf/rhubarb-lip-sync/blob/master/README.adoc)
