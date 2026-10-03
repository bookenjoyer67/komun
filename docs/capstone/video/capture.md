# Programmatic terminal capture, and how it is composited under the narration

The presenter records the video with a normal screen recorder. This file covers the fallback path:
producing a programmatic terminal capture for the segments whose only content is terminal output, so
those segments do not depend on the recorder catching a keystroke at the right moment.

The segments that suit a programmatic capture are G1, M3, M4, M5, M6 and D1. Everything else is a
slide, an editor scroll or the console, and the presenter records those directly.

Nothing in this file uses `ydotool`, `xdotool` or `wtype`, and nothing here raises a window or
synthesises input. Every command writes into the scratch directory and none of them writes inside the
repository.

## What is installed on this machine, verified 2026-10-01

| Tool | Status | Note |
|---|---|---|
| `asciinema` | NOT installed | Preferred recorder, used if installable. |
| `agg` | NOT installed | The cast to GIF renderer. Needed only if asciinema is used. |
| `script` | installed, util-linux 2.42.4 | The primary fallback. |
| `scriptreplay` | installed, util-linux 2.42.4 | Replays a timing file. Not needed for the video. |
| `tmux` | installed, 3.7c | The second fallback, and the console's own host. |
| `ffmpeg` | installed, n9.0.1 | The compositor. |
| `chromium` | installed, version 153 family | Renders the captured text to a PNG panel. |
| `node`, `npx` | installed, v26.7.0 and 11.19.0 | Not needed by the capture path. |
| `bat` | installed | A quick way to eyeball a transcript with colour on screen. |

## Working directory

Every command below writes to one scratch directory. Set it once per shell:

```
S=/home/computing/.hermes/profiles/dev/cache/scratch/video
mkdir -p "$S"
```

Do not write captures into the repository. They are not tracked and they do not belong in a commit.

## Path one: asciinema, if it can be installed

Install line, in the order to try them. On CachyOS or Arch the package is the fastest route:

```
sudo pacman -S --needed asciinema agg
```

If that package is unavailable, install the recorder through a self-contained toolchain so nothing
touches the system Python:

```
cargo install --locked asciinema
cargo install --locked agg
```

Or, if cargo is not the preferred route:

```
pipx install asciinema
```

Verify before recording:

```
asciinema --version && agg --version
```

If both print a version, this is the preferred path. Skip to "Trimming, path one" and ignore the
`script` sections. If either command is missing after trying the lines above, use path two.

## Path two: `script` from util-linux, the primary fallback

This is the fallback that is certain to work on this host, because `script` is present and was
exercised end to end while this file was written.

### The exact command line

```
script -q -T "$S/p7.timing" -c "./scripts/run-agent.sh implementer bash -c 'python3 /workspace/eval/red-team/rt_grant_widen_files.py'" "$S/p7.typescript"
```

Read that command left to right. `-q` keeps `script` from printing its own banner to the terminal.
`-T` names the timing file. `-c` gives the command to run, as one shell string. The last word is the
transcript file.

There is a real trap in the short form of the timing flag. On util-linux 2.42.4, `-t "$S/p7.timing"`
fails with:

```
script: unexpected number of arguments
Try 'script --help' for more information.
```

The short form takes its value attached, meaning `-tFILE` with no space, and it is deprecated. Use
the long form `-T FILE`, as written above, or the attached short form `-t"$S/p7.timing"`. Do not
spend recording time on this.

### What it writes

Two files.

- `p7.typescript`. The transcript. It is a first line naming the command, then the raw byte stream
  the command produced, then a last line recording the exit status. Lines carry carriage returns
  because a terminal transcript is a terminal stream, so `cat` will show `^M` at each line end.
- `p7.timing`. One pair of fields per write to the terminal: a floating point delay in seconds and a
  byte count. It is what makes a windowed trim possible, and it is small, tens of lines for a
  ten-second command.

For the command above, the observed sizes on this host were 1129 bytes for the transcript and 109
bytes for the timing file.

### Trimming it to the right segment

Two things need removing before the capture is usable: the wrapper's own first and last lines, and,
if the recording ran long, any window the presenter does not want.

The line based trim, which removes the wrapper lines and normalises the carriage returns. This is the
command that was run successfully while writing this file:

```
python3 -c 'raw=open("'"$S"'/p7.typescript","rb").read().replace(b"\r\n",b"\n"); lines=raw.split(b"\n"); keep=[l for l in lines if not (l.startswith(b"Script started on ") or l.startswith(b"Script done on "))]; open("'"$S"'/p7.trimmed.txt","wb").write(b"\n".join(keep)); print("lines in:",len(lines),"lines out:",len(keep))'
```

Observed output on this content: `lines in: 17 lines out: 15`. The two removed lines are the
`Script started on ...` header and the `Script done on ...` trailer.

A time windowed trim, when the capture is longer than the slot it has to fill. It reads the timing
file, computes the cumulative clock, and keeps only writes inside a window given in seconds. Change
`START` and `END` to the part of the run that matters:

```
python3 -c '
START, END = 0.8, 9.5
t = open("'"$S"'/p7.timing").read().split()
raw = open("'"$S"'/p7.trimmed.txt","rb").read()
durs = [float(t[i]) for i in range(0, len(t), 2)]
sizes = [int(t[i]) for i in range(1, len(t), 2)]
pos = 0; clock = 0.0; out = []
for d, s in zip(durs, sizes):
    chunk = raw[pos:pos+s]; pos += s; clock += d
    if START <= clock <= END:
        out.append(chunk)
open("'"$S"'/p7.window.txt","wb").write(b"".join(out))
print("kept", len(out), "writes between", START, "and", END, "seconds")
'
```

The two trims are applied in the order shown: strip the wrapper lines first, then window. Note that
the byte offsets in the timing file are against the raw stream, so the window trim must read the
transcript it was recorded with, not a rewritten one. If the window matters, apply the window step to
`p7.typescript` and strip the header line by line afterwards.

## Path three: `tmux capture-pane`, when a live pane is already available

This is the right tool for a long running command that is already sitting in a pane, and for the
console session. It takes a snapshot of the pane's contents rather than recording the stream.

```
tmux -L capdemo new-session -d -s cap -x 205 -y 54 "bash -lc 'cd /home/computing/komun && ./scripts/run-agent.sh --matrix; echo MATRIX-DONE; sleep 120'"
sleep 8
tmux -L capdemo capture-pane -p -e -S -400 -t cap > "$S/pane.txt"
tmux -L capdemo kill-server
```

Observed on 2026-10-01: 54 lines captured, the table rendered correctly, exit 0.

Three things matter here.

- `sleep 120` at the end of the command string keeps the session alive long enough to capture. A
  session whose command finishes immediately exits, and then `capture-pane` reports
  `no server running on /tmp/tmux-1000/capdemo`. That failure was observed and is the usual mistake.
- `-S -400` goes back 400 lines into the history, so a taller capture than the pane height is
  possible. Without it, `capture-pane` returns only the visible pane.
- `-e` keeps the pane's escape sequences, which preserves colour for a screenshot. Drop `-e` for a
  plain text capture. Add `-J` to join wrapped lines, which is worth doing when a table row is wider
  than the pane.

For the console specifically, do not capture `./console/open.sh` this way. It attaches to a session,
which takes over the terminal it is typed into. Use `--dump` and capture that, or record the console
with the normal screen recorder.

## Rendering the captured text to something `ffmpeg` can composite

`ffmpeg` composites images and video, not a text transcript. With asciinema the cast becomes an
animated GIF through `agg` and that is the input. With the `script` fallback, render the trimmed
transcript to a terminal styled PNG with headless Chromium, which is installed.

Build the page from the trimmed transcript and screenshot it. Verified on 2026-10-01: it produced a
1280 x 720 PNG, 77721 bytes.

```
printf '<!doctype html><html><body style="background:#0b0b0b;margin:0"><pre style="color:#e6e6e6;font:16px/1.35 monospace;padding:24px">%s</pre></body></html>' \
  "$(sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' "$S/p7.trimmed.txt")" > "$S/p7.html"
chromium --headless --no-sandbox --disable-gpu --hide-scrollbars --window-size=1280,720 --screenshot="$S/p7.png" "file://$S/p7.html"
```

The `sed` escaping matters. Without it, an angle bracket in the terminal output is parsed as HTML and
the panel silently loses text. Check the PNG before compositing, because a wrong panel is invisible
until the final render.

For a progressive reveal, which reads better than a static panel while the narration describes the
commands, render one PNG per line and give each one its own start time. The simplest stable version:
render two PNGs, one with the launcher block and one with the four `BLOCKED` lines, and cut between
them.

## Compositing it under the narration

The capture is an inset that appears over the recorder's video for a window of time, and the
narration continues underneath. Give each capture an entry time and an exit time that match the
script's Running column.

Map the script's timestamps to seconds, then to the `enable` window in the filter. Shot M3 starts at
3:40, which is 220 seconds, and runs 20 seconds, so its window is 220 to 240.

The command below was verified on 2026-10-01 against synthetic inputs and a real captured panel. It
scales the panel to 900 pixels wide, positions it 40 pixels from the bottom right, and shows it only
inside the window:

```
ffmpeg -y -i presenter.mp4 -loop 1 -i "$S/p7.png" \
  -filter_complex "[1:v]scale=900:-1[c];[0:v][c]overlay=W-w-40:H-h-40:enable='between(t,220,240)'" \
  -c:v libx264 -pix_fmt yuv420p -c:a copy out-m3.mp4
```

Notes on that command.

- `-c:a copy` leaves the presenter's narration untouched. Never re-encode the narration audio.
- The window is expressed in seconds on the presenter's timeline, so it must match the clock in the
  script, not the length of the capture.
- For a second capture in the same shot, chain another overlay in the same `filter_complex`, or run
  the command again against `out-m3.mp4` as the new input.
- If the panel is taller than the video, `scale=900:-1` keeps the aspect ratio and the overlay clips
  at the edge. `scale=900:506` forces a 16:9 inset instead.
- For a full frame capture rather than an inset, replace the overlay with a hard cut:
  `[0:v][1:v]concat` is wrong here, because it concatenates rather than replaces. Use the overlay
  with `overlay=0:0:enable='between(t,220,240)'` to place the panel over the whole frame.

If the presenter prefers the capture to be the main picture while the narration continues, with the
presenter's face or screen hidden, use the full frame form above and leave the audio as is.

## Smoke test: prove the whole capture path works, right now

Run this before the recording session. It exercises all four steps: record, trim, render, composite.
It uses the probe seven command, which is read-only with respect to the repository.

Step one, record. This assumes the pre-flight checklist in `demo-runbook.md` passed, including the
broker.

```
S=/home/computing/.hermes/profiles/dev/cache/scratch/video
mkdir -p "$S"
cd /home/computing/komun
script -q -T "$S/smoke.timing" -c "./scripts/run-agent.sh implementer bash -c 'python3 /workspace/eval/red-team/rt_grant_widen_files.py'" "$S/smoke.typescript"
```

Expected: exit 0, and the command's own output is echoed to the terminal as it runs, ending in four
lines that each begin with `BLOCKED`. Two files appear in `$S`.

Step two, trim.

```
python3 -c 'raw=open("'"$S"'/smoke.typescript","rb").read().replace(b"\r\n",b"\n"); lines=raw.split(b"\n"); keep=[l for l in lines if not (l.startswith(b"Script started on ") or l.startswith(b"Script done on "))]; open("'"$S"'/smoke.trimmed.txt","wb").write(b"\n".join(keep)); print("lines in:",len(lines),"lines out:",len(keep))'
```

Expected: `lines in: N lines out: N-2`, and `grep -c BLOCKED "$S/smoke.trimmed.txt"` prints `4`.

Step three, render.

```
printf '<!doctype html><html><body style="background:#0b0b0b;margin:0"><pre style="color:#e6e6e6;font:16px/1.35 monospace;padding:24px">%s</pre></body></html>' \
  "$(sed -e 's/&/\&amp;/g' -e 's/</\&lt;/g' -e 's/>/\&gt;/g' "$S/smoke.trimmed.txt")" > "$S/smoke.html"
chromium --headless --no-sandbox --disable-gpu --hide-scrollbars --window-size=1280,720 --screenshot="$S/smoke.png" "file://$S/smoke.html"
file "$S/smoke.png"
```

Expected: a line reading `bytes written to file .../smoke.png`, then
`PNG image data, 1280 x 720` from `file`. On the content used while writing this file the PNG was
77721 bytes.

Step four, composite. There is no presenter recording yet, so this uses the PNG against a synthetic
base, which is exactly the filter chain the real render uses.

```
cd "$S"
ffmpeg -y -loglevel error -f lavfi -i testsrc=size=1280x720:rate=25:duration=4 -loop 1 -i smoke.png \
  -filter_complex "[1:v]scale=900:-1[c];[0:v][c]overlay=W-w-40:H-h-40:enable='between(t,1,3)'" \
  -t 4 -c:v libx264 -pix_fmt yuv420p smoke-composite.mp4
ffprobe -v error -show_entries format=duration,size -of default=nw=1 smoke-composite.mp4
```

Expected: `duration=4.000000` and a size in the tens of kilobytes. Observed on 2026-10-01:
`duration=4.000000`, `size=88683`.

When all four steps pass, the capture path works end to end and the recording can start.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `script: unexpected number of arguments` | the deprecated `-t FILE` short form with a space | use `-T FILE` or `-t"FILE"` |
| The transcript's first line names the command, not the output | that is the `Script started on` wrapper | strip it, as in step two of the smoke test |
| `^M` at every line end | a terminal transcript carries carriage returns | the trim step normalises `\r\n` to `\n` |
| `no server running on /tmp/tmux-1000/...` | the tmux command finished and the session exited | append `; sleep 120` inside the session command |
| The PNG is missing text | an unescaped `<` or `&` in the terminal output | run the `sed` escaping, do not skip it |
| `ffmpeg` reports `No such filter: overlay` | an old or unusual build | `ffmpeg -filters | grep overlay` and use the installed build's spelling |
| The audio drifts after compositing | the narration was re-encoded | keep `-c:a copy` |
| `agg: no such file` | path one was chosen without installing it | fall back to path two, which needs only `script` |
| `asciinema: command not found` after a `pipx install` | `pipx`'s bin directory is not on `PATH` | use the cargo install line instead |

## Timeline mapping for the compositing step

Convert each shot's Running column to a start in seconds and add the shot's Target seconds to get the
end. The money shot windows, which are the ones that need a capture rather than a slide:

| Shot | Running | Start seconds | Capture shown until |
|---|---|---|---|
| M3, probe seven live | 3:40 | 220 | 240 |
| M4, probe ten live | 4:00 | 240 | 260 |
| M5, the mount read-back | part of M4 | 250 | 260 |
| G1, the live gate run | 1:50 | 110 | 165 |
| D1, the deterministic step | 5:55 | 355 | 405 |

Time each capture to the narration it sits under, not to the capture's own natural run. A capture
that is shorter than its window should be held on its final frame rather than padded with nothing,
which means using `-loop 1` on a PNG leaf or `tpad=stop_mode=clone:stop_duration=N` on a video leaf.
