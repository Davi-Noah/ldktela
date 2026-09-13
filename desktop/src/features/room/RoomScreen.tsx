import { useCallback, useEffect, useRef, useState } from 'react';
import { media } from '../../app/runtime';
import { CHROME_IDLE_MS } from '../../config';
import type { CaptureRequest } from '../../media/tracks';
import type { QualityChoice } from '../../store/media';
import { useMediaStore } from '../../store/media';
import { RoomBody } from './RoomBody';
import { RoomChrome } from './RoomChrome';
import { SharePicker } from './SharePicker';
import { VideoStage } from './VideoStage';

export function RoomScreen() {
  const containerRef = useRef<HTMLDivElement>(null);
  const watching = useMediaStore((state) => state.watching);
  const [picker, setPicker] = useState(false);
  const [fullscreen, setFullscreen] = useState(false);
  const [chromeVisible, setChromeVisible] = useState(true);
  // Mirrors the state so pointer moves do not touch React on every event.
  const chromeVisibleRef = useRef(true);

  const hasVideo = watching !== null;

  useEffect(() => {
    if (!hasVideo) {
      chromeVisibleRef.current = true;
      setChromeVisible(true);
      return;
    }
    const container = containerRef.current;
    if (container === null) {
      return;
    }
    let timer: ReturnType<typeof setTimeout> | null = null;
    const hide = () => {
      chromeVisibleRef.current = false;
      setChromeVisible(false);
    };
    const arm = () => {
      if (timer !== null) {
        clearTimeout(timer);
      }
      timer = setTimeout(hide, CHROME_IDLE_MS);
    };
    const onMove = () => {
      if (!chromeVisibleRef.current) {
        chromeVisibleRef.current = true;
        setChromeVisible(true);
      }
      arm();
    };
    arm();
    container.addEventListener('pointermove', onMove);
    return () => {
      if (timer !== null) {
        clearTimeout(timer);
      }
      container.removeEventListener('pointermove', onMove);
    };
  }, [hasVideo]);

  useEffect(() => {
    const onChange = () => {
      setFullscreen(document.fullscreenElement !== null);
    };
    document.addEventListener('fullscreenchange', onChange);
    return () => {
      document.removeEventListener('fullscreenchange', onChange);
    };
  }, []);

  const onToggleFullscreen = useCallback(() => {
    if (document.fullscreenElement !== null) {
      void document.exitFullscreen();
      return;
    }
    void containerRef.current?.requestFullscreen();
  }, []);

  const onShare = useCallback(() => {
    setPicker(true);
  }, []);

  const onConfirmShare = useCallback((request: CaptureRequest) => {
    setPicker(false);
    void media.startShare(request);
  }, []);

  const onStop = useCallback(() => {
    void media.stopShare();
  }, []);

  const onQuality = useCallback((choice: QualityChoice) => {
    media.setQuality(choice);
  }, []);

  return (
    <main ref={containerRef} className="relative h-full w-full overflow-hidden bg-surface-0">
      {/* Mounted for the life of the screen. Only the wrapper's visibility changes,
          so the decoder survives every layout change. */}
      <div hidden={!hasVideo} className="absolute inset-0 bg-stage">
        <VideoStage />
      </div>

      {!hasVideo && (
        <div className="absolute inset-0">
          <RoomBody onShare={onShare} onStop={onStop} />
        </div>
      )}

      {/* Chrome exists only over a picture. With no video the body already carries
          the same controls, and a permanent bar would be chrome for its own sake. */}
      {hasVideo && (
        <div hidden={!chromeVisible}>
          <RoomChrome
            onShare={onShare}
            onStop={onStop}
            onToggleFullscreen={onToggleFullscreen}
            onQuality={onQuality}
            fullscreen={fullscreen}
          />
        </div>
      )}

      {picker && (
        <SharePicker
          onCancel={() => {
            setPicker(false);
          }}
          onConfirm={onConfirmShare}
        />
      )}
    </main>
  );
}
