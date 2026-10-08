# Casting (M35)

## App integration

The player bar's Cast button opens a list of Google Cast and DLNA devices.
Opening the list starts mDNS and SSDP discovery; it refreshes every eight
seconds while open. Device names, protocol and speaker/TV/group icons come
from the discovery results. Close or Esc closes the list without ending
playback. With no current song, the list asks you to choose one first.

Choosing a device connects to the Default Media Receiver or AVTransport,
then stops the local audio deck and loads the current song at its current
position through the LAN relay. "Connecting…" changes to "Playing on
<device>" once the receiver reports playback, or "Connected to <device>"
while paused. A busy Cast receiver requires
an explicit Replace confirmation before LAUNCH. Encore does not use the
YouTube receiver or send signed googlevideo URLs to devices.

The same player bar and desktop commands control play/pause, seek, volume,
Next and Previous. The backend still owns the queue, shuffle, repeat and
autoplay. The remote deck polls status every half second. Cast's explicit
`IDLE/FINISHED`, or DLNA's transition to `STOPPED` after Play, advances the
queue by loading another relay source. Repeat one reloads the same song.
Smooth mixes and Audition stay local and are not used while casting.

Stop casting reads the final position, stops Encore's own receiver session
and reloads the same song locally at that position, preserving pause state.
A lost connection or failed command uses the last confirmed position. A replaced Cast app
is treated as lost and is not stopped. Old-session and old-song reports are
stamped and cannot advance a newer queue or change its position.

Ownership stays one-way:

```text
player bar / desktop commands
          |
          v
core backend: queue, resolver, local/remote handoff
          |
          v
remote I/O task: serialized controls, stamped status
          |
          v
encore-cast session: Cast v2 / DLNA + LAN relay
```

`encore-cast` owns network protocols and relay lifetime, with no GPUI
dependency. Core sends only device summaries and session state to the app.
Each remote song replaces the published relay source. Cast keeps the
resolver's normal audio preference; a DLNA renderer without WebM/Opus but
with MP4 support gets an AAC stream instead, without changing the local
stream cache. Unsupported formats fail back to local playback. The macOS
bundle declares its local-network purpose and `_googlecast._tcp` service.

### Safe local checks

Never run control checks against a household speaker or TV. The app's
`ENCORE_CAST_LOCAL_ONLY=1` mode bypasses browser credential discovery and
sign-in, filters the device list and checks every control endpoint again
before connecting. It accepts literal loopback or addresses assigned to
this computer, verified by a local socket bind. Hostnames, unspecified
addresses and remote LAN addresses are refused. SOAP clients do not follow
redirects. This also supports gmrender-resurrect, whose libupnp refuses the
loopback interface. Start your own instance with silent audio/video
`fakesink` outputs and select only its name and UUID.

Optional `ENCORE_CAST_TEST_ADDR=127.0.0.1:<port>` and
`ENCORE_DLNA_TEST_URL=http://<this-computer>:<port>/description.xml` are
accepted only in local-only mode. These point at stand-ins you started;
they do not permit a remote device. Without them, the real mDNS/SSDP scan
still runs, but its results are filtered to this computer.

`scripts/cast-ui-check.sh dark|light` creates fresh config, cache and app
runtime directories under the worktree's gitignored `artifacts/cast/`,
seeds `crates/app/fixtures/cast-session.json`, sets `ENCORE_FAKE_STREAM` to
the local silence fixture and disables update requests with
`ENCORE_UPDATE_FEED=http://127.0.0.1:9/releases`. Invoke it inside
`scripts/gpui-input.sh locked ...` and keep subsequent input and captures
inside that lock. Generate its audio fixture first:

```sh
ffmpeg -f lavfi -i anullsrc=r=48000:cl=stereo -t 180 \
  -c:a libopus -b:a 96k artifacts/cast/m35-silence.webm
```

Tests use loopback TLS Cast and SOAP/SSDP DLNA doubles. They fetch relay
bytes, verify control payloads, refuse busy/replaced Cast sessions, and
cover queue advance and local handoff. Headless GPUI tests exercise the
real picker and player bar with a fake backend. No automated test controls
real network devices or uses a signed-in stream. The authorized manual
Shield exception below is separate from those automated checks.

Known limits: AirPlay, transcoding, receiver-managed gapless queues and
DLNA event subscriptions are not implemented. DLNA codecs and seeking
remain renderer-dependent, particularly fragmented MP4. A long pause can
outlive a resolved URL; the current relay does not re-resolve an expired
source during a range request. The app must stay running while casting.
Windows and macOS runtime behavior still needs those platforms; the
stand-in and desktop checks here run on Linux.

The backend check example requires `ENCORE_CAST_LOCAL_ONLY=1`,
`ENCORE_FAKE_STREAM=<local file>` and the exact
`ENCORE_CAST_TEST_UDN=uuid:<your-renderer-uuid>`. Run
`cargo run -p encore-core --example cast_check` only against the renderer
you started. It refuses cast errors so a silent fallback to local playback
cannot count as a passing remote-control check. Use
`--gstout-audiopipe='fakesink sync=true'` and
`--gstout-videopipe='fakesink sync=true'` for gmrender. Plain `fakesink`
consumes the file as fast as possible and is unsuitable for queue/position
checks. DLNA song replacement sends Stop before SetAVTransportURI, and
seek success requires the reported position to reach the target within
UPnP's whole-second precision. An acknowledged but ineffective seek is
reported as a failed remote command.

### App verification on Linux, 2026-10-08

The strict backend-to-gmrender check passed all 17 steps, including SSDP,
remote pause/play/seek/volume, Next and Previous, natural EOF advancing the
queue, and a paused return to the same song at 42 seconds. gmrender ran as
our own separate process with real-time silent `fakesink sync=true` outputs.
Its control log confirmed the URI changes, seeks and 25% volume. The check
rejects any cast error instead of counting local fallback as success.

The app walkthrough used fresh config/cache/runtime directories and the
synthetic queue. The button, discovered device list and casting state were
captured and inspected in both dark and light. Stop casting returned the
paused song at 0:45. Terminating our renderer while paused at 0:50 returned
the same song locally at 0:50, still paused. Captures stay in gitignored
`artifacts/gpui/cast-{dark,light}-{button,devices,active}-final.png`; the
return/loss captures are there too. Discovery refreshes have a fixed-height
status row so they cannot move Stop casting under the pointer.

#### Authorized Shield-only pass

After the stand-in walkthrough, the maintainer explicitly allowed one
real-device pass on the NVIDIA Shield and still prohibited every Nest Mini
and other device. `ENCORE_CAST_SHIELD_ONLY=1` filters discovery and checks
the control boundary again. It accepts only a non-group Google Cast device
whose discovered name or model contains `SHIELD`. It also bypasses browser
credential discovery and sign-in. No other real device was controlled.

The discovered device was `SHIELD`, model `SHIELD Android TV`. Before the
pass its receiver-reported device volume was 1.0, unmuted, with no Cast app
running. The app used `ENCORE_FAKE_STREAM` with the locally generated
WebM/Opus silence file, never a signed-in or YouTube stream.

| Step | Observed result |
| --- | --- |
| Start casting the current song | Default Media Receiver loaded the relay URL; the Shield fetched the local WebM/Opus bytes and its position advanced. Initial setup took about five seconds. |
| Pause and resume | The player bar switched to Connected to SHIELD while paused, then Playing on SHIELD after resume. |
| Seek | A seek to about 45 seconds was reflected in the receiver-driven player bar. |
| Low volume | MEDIA_STATUS confirmed media volume 0.190, 19%. This is stream volume, separate from the device's original 100% level. |
| Next | Second local song started on SHIELD, made a new relay fetch and retained the 19% media volume. |
| Stop casting | The second song was paused and sought to 50.76 seconds. Stop casting returned it to the local Rust deck with `Start::Seconds(50.76)`, still paused. The inspected captures show 0:50 before and after. |
| Cleanup | Receiver volume was explicitly restored to 1.0, unmuted. A final receiver status reported no running Cast app. |

The pass ran from connection at 12:13:54 UTC to local return at 12:15:33
UTC, about 99 seconds, including pauses. All receiver-control steps passed;
none needed a retry. There was no audible test signal because the fixture
was silence. This verifies the Shield's WebM/Opus decoder and remote
controls, not listening quality or a live googlevideo stream.

The completion audit then found a local decoder issue that the silent
fixture and player clock alone could not reveal. The requested handoff was
50.76 seconds, but Symphonia 0.6.1's WebM seek returned a first packet at
54.975 seconds. Both Accurate and Coarse modes did this. The audio decoder
now uses a fresh reader while backing off until its anchor precedes the
requested time, or reopens at the start when the first cue is too late,
then trims decoded PCM to the target. A fresh reader also clears the
demuxer's old buffered packets. The synthetic audio fixtures cover 0.5,
6.76 and 50.76 seconds. A frequency-sweep PCM check proves that the actual
decoded content, not only the player clock, matches the requested time.
`cast_check --local-seek` rechecks only that native decoding path, with
local test audio and no casting commands. The Shield pass was not repeated.

`crates/cast/examples/shield_probe.rs` is the guarded read-only status and
explicit volume-restore helper. Its default invocation does not launch or
load anything. Captures and volume snapshots are in gitignored
`artifacts/gpui/cast-shield-*.png` and `artifacts/cast/shield-*.json`; device
addresses and session logs are not committed.

#### Requested live-song demo and picker fix

The maintainer then explicitly requested a live signed-out song on the
Shield. A fresh branch build ran with fresh XDG directories,
`ENCORE_CAST_SHIELD_ONLY=1`, the local disabled update feed and no
`ENCORE_FAKE_STREAM`. Get Lucky, Official Audio, by Daft Punk was resolved
signed out through VISIONOS as itag 251 and relayed to the Shield. Its
MEDIA_STATUS confirmed 28% media volume and the player bar showed Playing
on SHIELD. The maintainer confirmed that Default Media Receiver displayed
the song and worked. No other real device was controlled.

When the maintainer requested Stop, the cast and receiver app were closed,
the receiver's original 1.0 unmuted volume was restored, and the demo app
was quit so nothing continued locally. The cleanup status reported no Cast
app running. Live-demo captures and logs stay under gitignored `artifacts/`.

The reported no-op picker click was reproduced headlessly. When discovery
inserted another device during a held mouse click, index-based row IDs
changed even though the chosen row stayed under the pointer. Mouse release
then sent no Connect command. Rows now use protocol plus stable device ID.
The regression requires that held click to connect the same receiver, never
a newly appeared one. Accepted clicks show Connecting immediately and an
older discovery report cannot erase that feedback. A receiver missing from
the backend's refreshed catalogue returns a plain error instead of silently
ignoring the click. Frontend selections and backend requests are logged.

Final verification passed `just verify cast` with 34 tests, `just verify
core` with 72 tests, `just verify app` with 53 tests and `just verify audio`
with 15 tests. `just verify-workspace` passed all 210 workspace tests,
formatting, clippy and seven shaders. The Flatpak source generator was run
after the lockfile's core-to-cast dependency change; its output was unchanged.
The picker fix was also captured and inspected against our local renderer,
and all check apps and stand-ins were stopped afterward. No visuals or 3D
scene source was changed.

## Spike findings

How Encore can play to network speakers and TVs, what was proved on a real
network on 2026-10-08, and what the real feature should be.

**In short:** Google Cast through the Default Media Receiver works today,
with the app serving the stream from a small HTTP relay on the LAN. Two Nest
Mini speakers played every format Encore resolves (WebM/Opus like itags
251/774, fragmented MP4/AAC like 140/141), and seeked. DLNA uses the same relay and
worked end to end against a real renderer (gmrender-resurrect). AirPlay has
no Rust sender and is the most work; leave it for later. Don't use the
YouTube receiver.

## What was built

The original spike in `crates/cast`, package `encore-cast`, supplied:

| Module | What it does |
|---|---|
| `mdns` | Browses `_googlecast._tcp` with `mdns-sd` and reads the TXT keys `fn` (name), `md` (model), `id`, `rs` (what the device shows). |
| `ssdp`, `dlna` | `M-SEARCH` for `MediaRenderer:1`, the device description (quick-xml), AVTransport (SetAVTransportURI with DIDL-Lite, Play, Pause, Stop, Seek, GetTransportInfo, GetPositionInfo), RenderingControl SetVolume, ConnectionManager GetProtocolInfo. |
| `castv2` | A Cast v2 sender: TLS to port 8009 (rustls with ring, any certificate, handshake signature checked), `CastMessage` encoded by hand (the bytes match `protoc --encode`), heartbeat, requests matched to replies by `requestId`; GET_STATUS, LAUNCH, LOAD, PLAY/PAUSE/SEEK, media volume, STOP. |
| `relay` | An HTTP/1.1 server bound to the LAN address the device reaches us by (`local_ip_for`), serving each published stream under a random 128-bit path. A source is a file or a remote URL; a remote one is fetched as the device asks, with its `Range` passed upstream, so nothing is buffered in the app. Sends `Accept-Ranges`, the DLNA `transferMode`/`contentFeatures` headers and `Access-Control-Allow-Origin: *`. |

```sh
cargo run -p encore-cast --example cast_scan          # read-only device list
cargo run -p encore-cast --example cast_play -- [--seconds N] [--seek S] [--proxy] [--force] DEVICE FILE
just test cast                                        # fakes on 127.0.0.1
```

`cast_play` serves a local file (never a signed-in stream), plays it for a
few seconds, seeks, stops the app on the device and prints every request
the device made to the relay. It refuses a Cast device that is showing
something other than its idle screen unless given `--force`. `--proxy` puts a
second relay in front as a remote source: the path a googlevideo URL takes,
without YouTube.

The tests run against fakes: a TLS Cast receiver (self-signed certificate,
PING to the sender, LAUNCH/LOAD/SEEK/PAUSE/STOP, fetching the LOAD URL from
the relay), a DLNA renderer (SSDP answer, description with relative and
absolute control URLs, SOAP actions, a UPnP fault, fetching the URI on
Play), and the relay alone (ranges, HEAD, 416, unknown paths, a remote
source).

## What was tested on this network (2026-10-08, Fedora 43, KDE)

The network has three Cast devices (two Google Nest Mini speakers and an
Android TV box with Cast built in) and no DLNA renderer. SSDP only finds the
routers' Internet Gateway Device. For DLNA, gmrender-resurrect 0.3.1 (GPL,
run as a separate process from nixpkgs, GStreamer, silent `fakesink`) ran on
the LAN interface.

**Discovery.** `cast_scan` found the three Cast devices in 3 s with avahi
running: `mdns-sd` shares port 5353. The SSDP search found gmrender-resurrect
and read its Sink protocol info (audio/webm, audio/mp4, audio/ogg, audio/x-opus,
FLAC, L16 and many more).

**Cast to a Nest Mini.** Each file was 60 s long, made from the test
recording with ffmpeg. It played for 5 to 7 s, seeked, and was stopped.

| Format (as YouTube serves it) | Played | Seek | What the device fetched from the relay |
|---|---|---|---|
| WebM/Opus (251, 774) | yes | yes | one GET without `Range` (whole file), then on seek the Cues at the end and a range from the target |
| Fragmented MP4/AAC with `sidx` (140, 141) | yes | yes | a 4-byte probe at the end, the body from just after the header, then a range from the target |
| MP4/AAC, `moov` first | yes | yes | GET without `Range`, then a range from the target |
| Ogg/Opus | yes | yes | a range near the end, the body, seven small ranges from the end (finding the duration), then a range from the target |
| WebM/Opus through `--proxy` | yes | yes | the same ranges, passed upstream unchanged |

The receiver's user agent is `Cast Lite`. It opens a new connection per
range, sends no HEAD and no CORS preflight, and reads well ahead: the relay
had sent the whole 60 s file before the first `PLAYING` status arrived. LOAD answers `IDLE` first, then a
MEDIA_STATUS broadcast says `PLAYING`; the status reports the position
(`currentTime`) and moves with SEEK. Google's media page lists Opus only under
"Chromecast Audio / Google Home / Home Mini" and has no `audio/webm; codecs=opus`
MIME entry, so check WebM/Opus on a Chromecast and a TV before relying on it.

**DLNA to gmrender-resurrect.** WebM/Opus and Ogg/Opus played and seeked
(GStreamer's `souphttpsrc`, ranges like the Nest's). Fragmented MP4 played,
but Seek was accepted and did nothing. GStreamer can't seek that file when it
streams it. Renderers differ in this way, so the real feature should
prefer progressive files and check that the position moves after a seek.

**A googlevideo URL** (one signed-out `VISIONOS` resolve of a public song,
itag 251, plus 9 requests to googlevideo from this machine; the URL was never sent to a device):

| Check | Result |
|---|---|
| `Range: bytes=0-1023`, and a range in the middle | 206, `audio/webm`, `Accept-Ranges: bytes` |
| HEAD | 200 with `Content-Length` |
| GET without `Range` | 200, streams the whole file |
| A Cast receiver's user agent and `Origin: https://www.gstatic.com` | 206, but no `Access-Control-Allow-Origin` header |
| The `ip` parameter changed | 403 (`ip` is among the signed `sparams`) |
| Itag 140 | 302 to another googlevideo host |

**Not tested in the spike:** a direct googlevideo URL on a device (the spike plays no
YouTube streams), the Android TV (it would wake the TV), Cast groups, Sonos or
any TV's own renderer, AirPlay, IPv6, Windows and macOS. The later authorized
app pass above tested the Shield. Cross-checks for
Windows and macOS stop at ring's C build, which needs the MSVC and Apple
toolchains; CI has them.

## Google Cast

**Default Media Receiver** (app id `CC1AD845`): plays a URL it is given, no
registration. The audio codecs listed are FLAC, HE-AAC and LC-AAC, MP3,
Opus, Vorbis and WAV, in MP4, WebM, Ogg, MP3 and WAV containers
(<https://developers.google.com/cast/docs/media>). CORS is needed for adaptive
streaming (HLS, DASH), not for a plain progressive file, which matches what
the Nest did. LOAD takes `MusicTrackMediaMetadata` (title, artist, album,
cover URL), which the device and the Google Home app show.

**Direct URL or relay.** googlevideo URLs carry the requesting address in
the signed `ip` parameter. Behind the same NAT a speaker shares the
desktop's public IPv4, so a direct URL would probably play: catt sends
yt-dlp's URLs to the default receiver, and users report them playing on
Nest and Google Home (<https://github.com/skorokithakis/catt/issues/321>).
It breaks when the app resolved over IPv6 (every device has its own
address), when the URL expires during a long pause (about 6 hours), and for
anything the device fetches without the app's session. The relay avoids
all of that. It also serves local files, lets the app re-resolve without the
device noticing, and puts DLNA on the same path. Its costs: the app must
stay running and awake while casting, and LAN traffic about equal to the
bitrate (160–260 kbit/s). **Use the relay always.**

**The YouTube and YouTube Music receivers** (the official apps' path): the
sender pairs with the receiver through YouTube's undocumented "lounge" API
(`get_lounge_token_batch`, `bind`, `setPlaylist`) and the device fetches the
stream itself (casttube, MIT: <https://github.com/ur1katz/casttube>;
pychromecast's YouTube controller). Public videos need no account on the
device; library and private items need a credentials-transfer token from
the sender's session. The API changes without notice. On Google Home speakers
catt reports that the YouTube app isn't available, and the YouTube Music
receiver's app id isn't documented anywhere we found. Benefits would be
playback that continues without the app, and history on the account. Neither
outweighs driving a private, account-bound protocol from a third-party
client. **Not feasible for now.**

**Crates.**

- `rust_cast` 0.21.0 (MIT, released 2025-12-30): rustls 0.23 with
  aws-lc-rs, `protobuf` pinned to 3.7.2 with `protobuf-codegen` at build
  time, a blocking API on `std::net`, and its README says YouTube launching
  is broken.
- `cast-sender` 0.3 (MIT) uses async-native-tls, which means OpenSSL on Linux.
- `oxicast` 0.0.3 is young.
- The hand-written sender here is about 650 lines (connection, messages and
  protobuf) on the app's tokio runtime, with no protobuf toolchain. **Keep
  it.**
- `mdns-sd` 0.21.5 (MIT or Apache-2.0, released every few weeks, its own
  thread, no runtime needed): **use it.**

Device authentication (the `deviceauth` namespace) proves the device is
genuine to the sender, and is optional for a sender.

## AirPlay

- **AirPlay 1 audio (RAOP)**: RTSP control, RTP audio as ALAC or PCM, an
  AES key exchanged under Apple's RSA key, and NTP-style timing. AirPlay 2
  speakers still accept it: pyatv streams to HomePod mini, Apple TV 4K and
  AirPort Express (<https://pyatv.dev/documentation/supported_features/>).
  Apple TV 4 and later, and password-protected devices, want pairing first.
  A HomePod set to "Only people in this home" refuses it
  (<https://www.music-assistant.io/player-support/airplay/>).
- **AirPlay 2 proper**: HomeKit pair-setup/pair-verify (SRP, Ed25519,
  X25519, ChaCha20-Poly1305), transient pairing by default and PIN pairing
  where the device asks for it, buffered audio and PTP timing. OwnTone's
  sender has no FairPlay code, so a sender doesn't seem to need FairPlay
  (<https://github.com/owntone/owntone-server/blob/master/src/outputs/airplay.c>).
- **Implementations**: no maintained Rust sender on crates.io (the crates
  are receivers, and `airplay-rs` 0.0.1 is AGPL). philippe44's libraop
  (`raop_play`, MIT, C) is a RAOP sender that could be linked or ported.
  pyatv (MIT, Python) is the best reference. OwnTone (GPL-2.0) and Music
  Assistant (Apache-2.0) have AirPlay 2 senders. shairport-sync is a
  receiver. Porting or linking GPL code would make Encore's binary GPL, so
  only libraop, pyatv and Music Assistant are usable as sources.
- **Effort**: RAOP over libraop with an FFI wrapper: a week or two, plus
  ALAC encoding (Apple's encoder is Apache-2.0) or PCM. A Rust RAOP port
  with pairing: several weeks. AirPlay 2 with HomeKit pairing: more again.
  Discovery is `_raop._tcp` and `_airplay._tcp` over the same `mdns-sd`.
  None of this could be tested here (no AirPlay device).

## DLNA/UPnP

- Discovery is SSDP (`M-SEARCH` to 239.255.255.250:1900, answers by unicast
  to the sender's port), then the description XML for the AVTransport,
  RenderingControl and ConnectionManager control URLs. Control is SOAP.
  There is no push status without GENA eventing (SUBSCRIBE plus a callback
  server). Polling GetTransportInfo and GetPositionInfo every second, as
  `cast_play` does, is enough to start.
- Renderers fetch the URL themselves with their own player, so the same
  relay serves them. Formats vary per device: read ConnectionManager's Sink
  list and pick from it. AAC in MP4 is the safest choice (itags 141 and 140).
  The fallback is LPCM (`audio/L16` or WAV) from the audio engine's decoded
  PCM, at about 1.5 Mbit/s on the LAN, with no codec licence. TVs want
  `transferMode.dlna.org` and `contentFeatures.dlna.org` on the response and a
  DIDL-Lite `protocolInfo`; the relay and `didl()` send both.
- **Sonos** plays MP3, AAC/M4A, OGG Vorbis, FLAC, ALAC, WAV and AIFF up to
  48 kHz. Opus is not listed
  (<https://support.sonos.com/en/article/supported-audio-formats-for-sonos-music-library>).
  It takes plain `http:` URIs over AVTransport (SoCo's `play_uri`). Since a
  2025-08 security advisory Sonos tells S2 owners to switch UPnP off, which
  stops every local UPnP controller, this one included
  (<https://www.sonos.com/en-us/security-advisory-2025-0002>). So Sonos
  support over UPnP will shrink. Sonos's own cloud Control API is a separate
  project.
- Crates: `rupnp` 3.0 and `ssdp-client` 2.1 (MIT or Apache-2.0, tokio)
  would replace about 500 lines here (SSDP, description, SOAP, XML). Either is fine; the hand-written
  version has no extra dependencies.

## Discovery and the firewall on each OS

- **Linux**: tested. Fedora's FedoraWorkstation zone accepts TCP and UDP
  1025–65535, so the relay and SSDP replies get through, and mDNS answers
  arrived with avahi running. Distributions with a deny-incoming default
  (ufw when enabled) block both. The app should say so when a scan finds
  nothing or a device never fetches from the relay.
- **macOS**: multicast and Bonjour need Local Network permission (TN3179,
  <https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy>).
  Add `NSLocalNetworkUsageDescription` and `NSBonjourServices`
  (`_googlecast._tcp`, later `_raop._tcp`) to Info.plist. The multicast
  entitlement isn't needed outside iOS. The permission follows the code
  signature, so ad hoc signed builds are tracked unreliably. If the
  application firewall is on, the relay's listening socket triggers its
  "accept incoming connections" prompt.
- **Windows**: the first listening socket without an allow rule makes
  Defender Firewall ask. A non-admin user's answer creates block rules, and
  so does declining. The installer should add inbound allow rules for
  `encore-yt.exe` on the Private profile
  (<https://learn.microsoft.com/windows/security/operating-system-security/network-security/windows-firewall/rules>).
  `mdns-sd` supports Windows.
- **Flatpak** (M36): `--share=network` covers multicast and the relay. No
  portal is involved.

## Recommendation for the feature

1. **Cast first**: Default Media Receiver plus the relay, built on
   `encore-cast`. **DLNA next**, on the same relay, with the format chosen
   from the renderer's Sink list. **AirPlay later**, as RAOP over libraop or
   a port of it. **No YouTube receiver.**
2. **Player bar**: a cast button that opens a device list (name, a speaker,
   TV or group icon, scanned when the list opens and kept fresh in the
   background while it's open). Picking a device shows "Connecting…", then
   "Playing on Kitchen speaker" in the player bar. If a Cast device is
   showing another app, ask before taking it over.
3. **The app as remote**: local playback pauses and the device's position
   drives the seek bar. Play/pause, seek and the stream's volume map to
   PLAY, PAUSE, SEEK and SET_VOLUME (AVTransport and RenderingControl on
   DLNA). The next song is a new LOAD when MEDIA_STATUS reports
   `IDLE`/`FINISHED`; QUEUE_LOAD with the next item would close the gap
   later. Stopping casting (or the device going away) STOPs the app on the
   device and resumes locally at the device's last position.
4. **The relay's source**: a resolver handle, not a fixed URL, so an expired or
   refused URL is resolved again and the device's next range request
   continues. Prefer the format the device plays without transcoding: Opus
   for Cast as now, AAC for DLNA without Opus. Keep the machine awake while
   casting (the same inhibitor as playback).
5. **Architecture**: `encore-cast` stays a backend crate with no UI. The
   backend gets a cast service next to `player::Player`, with commands
   (scan, connect, disconnect) and events (devices, status). The queue and
   Up next stay where they are, and only the deck changes.
6. **Effort**: Cast with the relay and the player bar UI, about a week. DLNA
   on top, 3–4 days (format choice, eventing or polling, quirks). Firewall
   rules and macOS Info.plist keys, 1–2 days with the packaging. RAOP, 2–4
   weeks.
7. **Risks**: device quirks (seeking in fragmented MP4 on some renderers),
   firewall prompts, several interfaces (`local_ip_for` picks the one with
   the route to each device), IPv6 link-local-only devices, and Sonos
   dropping UPnP. The relay serves only published streams under
   unguessable paths, bound to the LAN address, and the stream URLs and
   cookies never leave the app.
