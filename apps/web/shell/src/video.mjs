// Videos and sound clips as Locora keeps them: in the codec they were recorded in, when devices
// commonly play it (the same list as crates/locora-media). Such a file is handed to the core as it
// is, which removes the metadata without re-encoding. Only a file in another codec (ProRes, MPEG-4
// Part 2, AMR …) is converted here with WebCodecs (mediabunny, loaded only when needed); a browser
// that cannot read or encode it says so.

const VIDEO_NAME = /\.(mp4|m4v|mov|webm|mkv|3gp)$/i;
/** Sound files the core checks itself: their containers hold no codec outside the list. */
const PLAIN_SOUND = /\.(mp3|wav|ogg|oga|opus|flac|aac)$/i;

/** Codecs kept as they are, by container. */
const KEPT = {
  isobmff: { video: ['avc', 'hevc', 'av1', 'vp9'], audio: ['aac', 'opus', 'mp3', 'alac', 'flac'] },
  webm: { video: ['vp8', 'vp9', 'av1', 'avc', 'hevc'], audio: ['opus', 'vorbis', 'aac', 'flac'] },
};

export function isVideo(file) {
  return file.type.startsWith('video/') || VIDEO_NAME.test(file.name);
}

/** The container family of a mediabunny input: `isobmff`, `webm`, or null for any other. */
async function family(mb, input) {
  const format = await input.getFormat();
  if (format instanceof mb.IsobmffInputFormat) return 'isobmff';
  // Matroska files are kept only with the WebM document type, which browsers play.
  if (mb.WebMInputFormat && format instanceof mb.WebMInputFormat) return 'webm';
  return null;
}

/** Whether every track of the input is in a codec Locora keeps for its container. */
async function kept(mb, input, soundOnly) {
  const container = await family(mb, input);
  if (!container) return false;
  const videos = await input.getVideoTracks();
  const sounds = await input.getAudioTracks();
  if (soundOnly ? videos.length > 0 || sounds.length === 0 : videos.length === 0) return false;
  return (
    videos.every((track) => KEPT[container].video.includes(track.codec)) &&
    sounds.every((track) => KEPT[container].audio.includes(track.codec))
  );
}

function open(mb, file) {
  return new mb.Input({ source: new mb.BlobSource(file), formats: mb.ALL_FORMATS });
}

/**
 * The video as Locora keeps it: the file itself when its codecs are on the list (no conversion),
 * else a copy converted to MP4 (H.264) or, where the browser has no H.264 encoder, WebM (VP9).
 */
export async function videoAsKept(file) {
  const mb = await import('./mediabunny.mjs');
  const input = open(mb, file);
  let video;
  try {
    video = await input.getPrimaryVideoTrack();
  } catch {
    throw 'Dieses Videoformat kann der Browser nicht lesen. Bitte als MP4, MOV oder WebM speichern.';
  }
  if (!video) throw 'Im Video wurde keine Bildspur gefunden.';
  if (await kept(mb, input, false)) return file;
  if (!(await video.canDecode())) {
    throw `Dieses Video (Format ${video.codec ?? 'unbekannt'}) kann der Browser weder abspielen noch umwandeln. Füge es in der Android-App oder am Computer mit ffmpeg hinzu, oder speichere es vorher als MP4 (H.264).`;
  }
  const target = (await mb.canEncodeVideo('avc'))
    ? { format: new mb.Mp4OutputFormat({ fastStart: 'in-memory' }), video: 'avc', type: 'video/mp4', extension: 'mp4' }
    : (await mb.canEncodeVideo('vp9'))
      ? { format: new mb.WebMOutputFormat(), video: 'vp9', type: 'video/webm', extension: 'webm' }
      : null;
  if (!target) throw 'Dieser Browser kann keine Videos umwandeln. Speichere das Video vorher als MP4 (H.264) oder WebM.';
  const audio = target.extension === 'mp4' && (await mb.canEncodeAudio('aac')) ? 'aac' : 'opus';
  const output = new mb.Output({ format: target.format, target: new mb.BufferTarget() });
  const conversion = await mb.Conversion.init({
    input,
    output,
    tracks: 'primary',
    video: { codec: target.video },
    audio: { codec: audio },
    tags: {},
    showWarnings: false,
  });
  const lostPicture = conversion.discardedTracks.some((entry) => entry.track.isVideoTrack());
  if (!conversion.isValid || lostPicture) {
    throw 'Dieser Browser kann das Video nicht umwandeln. Füge es in der Android-App hinzu oder speichere es vorher als MP4 (H.264) oder WebM.';
  }
  await withProgress('Video wird umgewandelt', conversion);
  const name = `${file.name.replace(/\.[^.]+$/, '')}.${target.extension}`;
  return new File([output.target.buffer], name, { type: target.type });
}

/**
 * A sound clip as Locora keeps it: the file itself when its codec is on the list, else a copy
 * converted to M4A (AAC) or, without an AAC encoder, Ogg (Opus). Formats the browser cannot read
 * (CAF, AMR) give a clear message.
 */
export async function soundAsKept(file) {
  if (PLAIN_SOUND.test(file.name)) return file;
  const mb = await import('./mediabunny.mjs');
  const input = open(mb, file);
  let sound;
  try {
    sound = await input.getPrimaryAudioTrack();
  } catch {
    sound = null;
  }
  const unreadable = 'Dieses Tonformat kann der Browser nicht lesen. Bitte die Aufnahme als MP3, M4A, WAV, Ogg, WebM oder FLAC speichern.';
  if (!sound) throw unreadable;
  if (await kept(mb, input, true)) return file;
  if (!(await sound.canDecode())) throw unreadable;
  const aac = await mb.canEncodeAudio('aac');
  if (!aac && !(await mb.canEncodeAudio('opus'))) throw unreadable;
  const output = new mb.Output({
    format: aac ? new mb.Mp4OutputFormat({ fastStart: 'in-memory' }) : new mb.OggOutputFormat(),
    target: new mb.BufferTarget(),
  });
  const conversion = await mb.Conversion.init({
    input,
    output,
    tracks: 'primary',
    video: { discard: true },
    audio: { codec: aac ? 'aac' : 'opus' },
    tags: {},
    showWarnings: false,
  });
  if (!conversion.isValid) throw unreadable;
  await withProgress('Ton wird umgewandelt', conversion);
  const name = `${file.name.replace(/\.[^.]+$/, '')}.${aac ? 'm4a' : 'ogg'}`;
  return new File([output.target.buffer], name, { type: aac ? 'audio/mp4' : 'audio/ogg' });
}

async function withProgress(label, conversion) {
  const progress = showProgress(label);
  conversion.onProgress = (share) => progress.update(share);
  try {
    await conversion.execute();
  } finally {
    progress.close();
  }
}

/** A small notice while a file is converted; it can take a while for a long video. */
function showProgress(label) {
  const box = document.createElement('div');
  box.setAttribute('role', 'status');
  box.style.cssText =
    'position:fixed;left:50%;bottom:24px;transform:translateX(-50%);z-index:1001;padding:10px 16px;border-radius:10px;background:#16173a;color:#fff;font:inherit;box-shadow:0 4px 16px rgba(0,0,0,.3)';
  box.textContent = `${label} …`;
  document.body.append(box);
  return {
    update(share) {
      box.textContent = `${label} … ${Math.round(share * 100)} %`;
    },
    close() {
      box.remove();
    },
  };
}
