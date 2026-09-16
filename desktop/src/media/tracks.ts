import { VideoQuality } from 'livekit-client';
import type { RemoteTrackPublication } from 'livekit-client';
import type { QualityChoice } from '../store/media';

/**
 * The viewer's half of media. It acquires nothing.
 *
 * Capture, encoding and publishing moved to the Rust core (ADR-0026), and with
 * them the encoding ladder — it belongs next to the encoder. What is left here
 * is the one decision a viewer actually makes: which of the layers arriving to
 * ask for.
 */

/**
 * RF-19, and the viewer half of ADR-0023.
 *
 * `auto` leaves `adaptiveStream` in charge, which is what keeps a grid of N
 * screens from costing N full streams (RF-32): a video rendered small gets the
 * small layer on its own. `high` overrides that heuristic by asking for the
 * published dimensions, which is the only way to get the top layer into a
 * thumbnail-sized element. `low` caps at the bottom layer.
 *
 * There is no frame-rate choice here on purpose: frame rate belongs to the
 * publisher, and a selector offering one would be offering something nobody is
 * sending.
 */
export function applyQuality(publication: RemoteTrackPublication, choice: QualityChoice): void {
  switch (choice) {
    case 'auto':
      publication.setVideoQuality(VideoQuality.HIGH);
      return;
    case 'high': {
      const published = publication.dimensions;
      if (published !== undefined) {
        publication.setVideoDimensions(published);
      }
      publication.setVideoQuality(VideoQuality.HIGH);
      return;
    }
    case 'low':
      publication.setVideoQuality(VideoQuality.LOW);
      return;
  }
}
