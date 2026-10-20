// Videos as Locora stores them: MP4 with H.264, and AAC or Opus sound. A video in another format
// or codec is converted here with WebCodecs (mediabunny, loaded only when a video is added); the
// core then removes the metadata. A browser that cannot read the video says so.

const VIDEO_NAME = /\.(mp4|m4v|mov|webm|mkv)$/i;

export function isVideo(file) {
  return file.type.startsWith('video/') || VIDEO_NAME.test(file.name);
}

/** The video as MP4 with H.264: the file itself when it is one, else a converted copy. */
export async function asMp4(file) {
  const mb = await import('./mediabunny.mjs');
  const input = new mb.Input({ source: new mb.BlobSource(file), formats: mb.ALL_FORMATS });
  const video = await input.getPrimaryVideoTrack();
  if (!video) throw 'Im Video wurde keine Bildspur gefunden.';
  const tracks = await input.getTracks();
  const audio = await input.getPrimaryAudioTrack();
  const isobmff = (await input.getFormat()) instanceof mb.IsobmffInputFormat;
  const ready =
    isobmff &&
    video.codec === 'avc' &&
    (!audio || audio.codec === 'aac' || audio.codec === 'opus') &&
    tracks.length === (audio ? 2 : 1);
  if (ready) return file;
  if (!(await video.canDecode())) {
    throw 'Dieses Video kann der Browser nicht lesen (häufig HEVC/H.265 vom Handy) und darum nicht in MP4 umwandeln. Füge es in der Android-App hinzu oder speichere es vorher als MP4 (H.264).';
  }
  const audioCodec = (await mb.canEncodeAudio('aac')) ? 'aac' : 'opus';
  const output = new mb.Output({ format: new mb.Mp4OutputFormat({ fastStart: 'in-memory' }), target: new mb.BufferTarget() });
  const conversion = await mb.Conversion.init({
    input,
    output,
    tracks: 'primary',
    video: { codec: 'avc' },
    audio: { codec: audioCodec },
    tags: {},
    showWarnings: false,
  });
  const lostPicture = conversion.discardedTracks.some((entry) => entry.track.isVideoTrack());
  if (!conversion.isValid || lostPicture) {
    throw 'Dieser Browser kann das Video nicht in MP4 (H.264) umwandeln. Füge es in der Android-App hinzu oder speichere es vorher als MP4 (H.264).';
  }
  const progress = showProgress();
  conversion.onProgress = (share) => progress.update(share);
  try {
    await conversion.execute();
  } finally {
    progress.close();
  }
  const name = `${file.name.replace(/\.[^.]+$/, '')}.mp4`;
  return new File([output.target.buffer], name, { type: 'video/mp4' });
}

/** A small notice while a video is converted; it can take a while for a long video. */
function showProgress() {
  const box = document.createElement('div');
  box.setAttribute('role', 'status');
  box.style.cssText =
    'position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:1001;padding:10px 16px;border-radius:10px;background:#16173a;color:#fff;font:inherit;box-shadow:0 4px 16px rgba(0,0,0,.3)';
  box.textContent = 'Video wird in MP4 umgewandelt …';
  document.body.append(box);
  return {
    update(share) {
      box.textContent = `Video wird in MP4 umgewandelt … ${Math.round(share * 100)} %`;
    },
    close() {
      box.remove();
    },
  };
}
