package cloud.locora.video

import android.app.Activity
import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.transformer.Composition
import androidx.media3.transformer.EditedMediaItem
import androidx.media3.transformer.EditedMediaItemSequence
import androidx.media3.transformer.ExportException
import androidx.media3.transformer.ExportResult
import androidx.media3.transformer.Transformer
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.Plugin
import java.io.File

@InvokeArg
class ConvertArgs {
  lateinit var input: String
  lateinit var output: String
}

/**
 * Converts a video into MP4 with H.264 and AAC, with the device's encoders. HDR is mapped to SDR,
 * so every browser shows it. An H.264 video with AAC is only re-packed, which is quick.
 */
@TauriPlugin
class VideoPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun convert(invoke: Invoke) {
    val args = invoke.parseArgs(ConvertArgs::class.java)
    // The transformer runs on a thread with a looper and reports there.
    Handler(Looper.getMainLooper()).post {
      try {
        val item = EditedMediaItem.Builder(MediaItem.fromUri(Uri.fromFile(File(args.input)))).build()
        val composition = Composition.Builder(EditedMediaItemSequence.Builder(item).build())
          .setHdrMode(Composition.HDR_MODE_TONE_MAP_HDR_TO_SDR_USING_OPEN_GL)
          .build()
        val transformer = Transformer.Builder(activity)
          .setVideoMimeType(MimeTypes.VIDEO_H264)
          .setAudioMimeType(MimeTypes.AUDIO_AAC)
          .addListener(object : Transformer.Listener {
            override fun onCompleted(composition: Composition, exportResult: ExportResult) {
              invoke.resolve()
            }

            override fun onError(composition: Composition, exportResult: ExportResult, exportException: ExportException) {
              invoke.reject("Das Video konnte nicht in MP4 (H.264) umgewandelt werden: ${exportException.message}")
            }
          })
          .build()
        transformer.start(composition, args.output)
      } catch (error: Exception) {
        invoke.reject("Das Video konnte nicht in MP4 (H.264) umgewandelt werden: ${error.message}")
      }
    }
  }
}
