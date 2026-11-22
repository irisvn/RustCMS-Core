// Opening the image viewer: what the places that show images use. The viewer
// itself (./image-viewer, with its library and stylesheets) loads the first
// time one opens, so pages without images never download it.
import { Component, type ReactNode, Suspense, lazy, useState } from "react";
import type { MediaItem } from "@/api/types";

/** An image of the media library, with what the caller knows of it. */
export interface ViewerImage {
  /** Media id. */
  id: string;
  name?: string;
  alt?: string;
  /** Bytes. */
  size?: number;
  width?: number;
  height?: number;
}

/** What the viewer knows of a media item. */
export const viewerImageOf = ({ id, name, alt, size, width, height }: MediaItem): ViewerImage => ({ id, name, alt, size, width, height });

const ImageViewer = lazy(() => import("./image-viewer"));

/**
 * The viewer's chunk may fail to load (a deploy replaced it, the network
 * dropped), or the viewer fail while open: it closes and the page under it
 * stays, with its drafts. Opening it again until the page reloads closes it
 * the same way (`lazy` keeps the failed import; retrying it could bring the
 * script back without its stylesheet).
 */
class ViewerBoundary extends Component<{ onClose: () => void; children: ReactNode }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown) {
    console.warn("rustcms: the image viewer could not be opened", error);
    this.props.onClose();
  }

  render() {
    return this.state.failed ? null : this.props.children;
  }
}

/**
 * A viewer over `images` and how to open it: `open(i)` shows the `i`-th. It
 * closes by itself when the images change (another page, a refetch with other
 * files), rather than show another image at the same place.
 */
export function useImageViewer(images: ViewerImage[]): { open: (index: number) => void; viewer: ReactNode } {
  const ids = images.map((image) => image.id).join(",");
  const [opened, setOpened] = useState<{ index: number; ids: string } | null>(null);
  const current = opened !== null && opened.ids === ids && opened.index < images.length ? opened : null;
  if (opened !== null && current === null) setOpened(null);
  return {
    open: (index) => {
      if (index >= 0 && index < images.length) setOpened({ index, ids });
    },
    // Mounted while open only (its media lookups too).
    viewer: current && (
      <ViewerBoundary onClose={() => setOpened(null)}>
        <Suspense fallback={null}>
          <ImageViewer images={images} index={current.index} onClose={() => setOpened(null)} />
        </Suspense>
      </ViewerBoundary>
    ),
  };
}
