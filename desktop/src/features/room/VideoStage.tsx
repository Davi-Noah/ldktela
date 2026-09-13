import { useEffect, useRef } from 'react';
import { media } from '../../app/runtime';

/**
 * The media elements are created once for the whole room screen and handed to the
 * media session, which attaches and detaches tracks on them. They are never
 * conditionally rendered and never move in the tree: remounting a `<video>` tears
 * the decoder down and costs seconds of black frames (CLAUDE.md §7).
 *
 * Visibility is the parent's job, through a wrapper. Hiding the wrapper is also
 * what tells adaptiveStream to stop pulling layers (RF-16).
 */
export function VideoStage() {
  const videoRef = useRef<HTMLVideoElement>(null);
  const audioRef = useRef<HTMLAudioElement>(null);

  useEffect(() => {
    media.registerVideoElement(videoRef.current);
    media.registerAudioElement(audioRef.current);
    return () => {
      media.registerVideoElement(null);
      media.registerAudioElement(null);
    };
  }, []);

  return (
    <>
      <video
        ref={videoRef}
        autoPlay
        playsInline
        muted
        disablePictureInPicture
        className="h-full w-full object-contain"
      />
      {/* Screen audio rides its own element so the video can stay muted and never
          trip the autoplay policy. */}
      <audio ref={audioRef} autoPlay />
    </>
  );
}
