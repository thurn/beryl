# Working in `kit_base`

This is the toolkit's vocabulary: geometry, poses, paint, typography, design
tokens, asset addresses and validated tuning. Keep Magic and rendering backends
out of it. The host supplies data to the client.

Start at `data::Data` and `data::read` for tuning, `geometry` and `pose::Pose`
for coordinates and motion, or `paint` and `atlas` for drawing inputs. Validate
finite values and ranges at the data boundary. The build script generates
`tokens` from `data/design-tokens.json` and embeds the serif fonts.

`tests/all/` exercises data parsing and tokens. Paint helpers have unit tests
beside their source. A tuning field can break both embedded startup data and
checkout overrides. Keep both paths valid and run `just client data`.
