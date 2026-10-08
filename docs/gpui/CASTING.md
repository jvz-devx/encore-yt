# Casting spike (M35)

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

`crates/cast` (package `encore-cast`, not linked into the app yet):

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

**Not tested:** a direct googlevideo URL on a device (the spike plays no
YouTube streams), the Android TV (it would wake the TV), Cast groups, Sonos or
any TV's own renderer, AirPlay, IPv6, Windows and macOS. Cross-checks for
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
