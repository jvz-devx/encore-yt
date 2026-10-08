# Synthetic seek fixture

`cast-seek.webm` is 60 seconds of locally generated stereo silence, not an
account or stream capture. Its WebM cues reproduce a seek landing after the
requested local cast-handoff position in Symphonia 0.6.1.

Generated with:

```sh
ffmpeg -f lavfi -i anullsrc=r=48000:cl=stereo -t 60 \
  -c:a libopus -b:a 96k crates/audio/tests/fixtures/cast-seek.webm
```

The test verifies packet positions near the start and at 6.76 and 50.76
seconds. Decoding must start before the target so the existing PCM trim can
reach it without skipping the requested audio.

`cast-seek-chirp.webm` is a synthetic frequency sweep, decoded only in the
offline unit test, never played on a device. Its instantaneous frequency is
`200 + 20 * t` Hz. The test measures the decoded PCM's frequency after a
50.76-second seek, so a correct player clock alone cannot hide wrong audio.

```sh
ffmpeg -f lavfi -i 'aevalsrc=0.2*sin(2*PI*(200*t+10*t*t)):s=48000:d=60' \
  -c:a libopus -b:a 24k -vbr off crates/audio/tests/fixtures/cast-seek-chirp.webm
```
