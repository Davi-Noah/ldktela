import { DisconnectReason, VideoQuality } from 'livekit-client';
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

/**
 * Whether a disconnect from the SFU is worth retrying.
 *
 * Everything is, except one. `DUPLICATE_IDENTITY` means the room kicked us
 * because the same account joined from somewhere else — a second machine, or a
 * second copy of the app. Reconnecting then kicks *them*, which makes them
 * reconnect and kick us, and the two clients trade the room about once a second
 * for as long as both are open. Nobody watches anything, and the publisher's
 * encoder is paused the whole time because it never sees a stable subscriber.
 *
 * Observed exactly that way: five joins and four `DUPLICATE_IDENTITY` removals
 * in four seconds, with `peak_viewers` stuck at zero.
 */
export function shouldRejoin(reason: DisconnectReason | undefined): boolean {
  return reason !== DisconnectReason.DUPLICATE_IDENTITY;
}

/** Said to the user, because the loop is otherwise indistinguishable from a bug. */
export const DUPLICATE_IDENTITY_MESSAGE =
  'Esta conta entrou na sala de outro lugar. O ldkcord só funciona em um computador por vez — feche o outro e entre de novo no canal de voz.';
