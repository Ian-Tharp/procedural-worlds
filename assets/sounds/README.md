# Sound Assets

Place `.ogg` audio files here for the audio system.

## Required Files

### Block Interaction Sounds
- `block_place.ogg` — Played when the player places a block
- `block_break.ogg` — Played when the player breaks a block

### Biome Ambient Loops
- `ambient_plains.ogg` — Ambient loop for Plains biome (wind, birds)
- `ambient_desert.ogg` — Ambient loop for Desert biome (wind, sand)
- `ambient_forest.ogg` — Ambient loop for Forest biome (birds, rustling)
- `ambient_mountains.ogg` — Ambient loop for Mountains biome (wind, echo)
- `ambient_tundra.ogg` — Ambient loop for Tundra biome (cold wind, ice)
- `ambient_volcanic.ogg` — Ambient loop for Volcanic biome (rumbling, fire)

## Notes

- All files should be in OGG Vorbis format (`.ogg`)
- Ambient loops should be seamlessly loopable
- Block sounds should be short one-shot effects (< 1 second)
- Missing files are handled gracefully — the system logs a warning but continues
