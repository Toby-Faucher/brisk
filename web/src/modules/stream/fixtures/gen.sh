#!/bin/sh
# Regenerates the replay fixtures (replay.test.ts): 3 s Annex B H.264 and HEVC at 60 fps, a key
# frame every second, parameter sets repeated on every key frame and an AUD per access unit (like
# Sunshine), H.264 (Baseline, and High as Sunshine sends) with a 1 s 256x144 tail on the Baseline one for the resolution-change test; and 3 s of 5 ms CBR
# stereo Opus (Sunshine's settings) in Ogg.
set -e
cd "$(dirname "$0")"
x264="-c:v libx264 -preset ultrafast -tune zerolatency -crf 38 -g 60 -bf 0 -x264-params aud=1:repeat-headers=1"
ffmpeg -v error -y -f lavfi -i testsrc2=size=320x180:rate=60 -t 3 $x264 a.h264
ffmpeg -v error -y -f lavfi -i testsrc2=size=256x144:rate=60 -t 1 $x264 b.h264
cat a.h264 b.h264 > h264.h264
rm a.h264 b.h264
ffmpeg -v error -y -f lavfi -i testsrc2=size=320x180:rate=60 -t 3 -c:v libx264 -profile:v high -preset veryfast -tune zerolatency \
  -crf 38 -g 60 -bf 0 -x264-params aud=1:repeat-headers=1 high.h264
ffmpeg -v error -y -f lavfi -i testsrc2=size=320x180:rate=60 -t 3 -c:v libx265 -preset ultrafast -tune zerolatency -crf 38 \
  -x265-params keyint=60:min-keyint=60:open-gop=0:bframes=0:aud=1:repeat-headers=1:log-level=error hevc.hevc
ffmpeg -v error -y -f lavfi -i sine=f=440:r=48000 -ac 2 -t 3 -c:a libopus -application lowdelay -frame_duration 5 \
  -b:a 128k -vbr off opus.ogg
