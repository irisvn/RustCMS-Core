// The image viewer (yet-another-react-lightbox), loaded when one first opens
// (`useImageViewer`, ./lightbox): images of the media library over the page,
// with zoom, the place's other images (←/→), the file's details and a
// download. It renders where menus do: inside an open <dialog>, else the body.
// Static stylesheets only, and styles set through the CSSOM: the CSP's
// `style-src 'self'` holds.
import { useState } from "react";
import Lightbox, { type Slide } from "yet-another-react-lightbox";
import Captions from "yet-another-react-lightbox/plugins/captions";
import Counter from "yet-another-react-lightbox/plugins/counter";
import Download from "yet-another-react-lightbox/plugins/download";
import Zoom from "yet-another-react-lightbox/plugins/zoom";
import "yet-another-react-lightbox/styles.css";
import "yet-another-react-lightbox/plugins/captions.css";
import "yet-another-react-lightbox/plugins/counter.css";
import { usePanelConfig } from "@/api/context";
import { useRecord } from "@/api/queries";
import { formatCount, useT } from "@/i18n";
import { mediaUrl } from "@/lib/rich-text";
import { mediaItemOf } from "@/renderers/media-picker";
import { type ViewerImage, viewerImageOf } from "./lightbox";
import { usePortalContainer } from "./ui/portal";

/** What the viewer shows of an image: the caller's, else its media record's (fetched when shown). */
function useDetails(image: ViewerImage | undefined): ViewerImage | undefined {
  const known = image && image.name !== undefined && image.size !== undefined;
  const record = useRecord("media", image?.id ?? "", { enabled: !!image && !known });
  if (!image || known || !record.data) return image;
  const found = viewerImageOf(mediaItemOf(record.data));
  // The caller's first, the record's for what it lacks.
  return {
    ...image,
    name: image.name ?? found.name,
    alt: image.alt || found.alt,
    size: image.size ?? found.size,
    width: image.width ?? found.width,
    height: image.height ?? found.height,
  };
}

/**
 * The viewer over `images`, open at `index`. Closing (Escape, the close
 * button, a click outside) calls `onClose`.
 */
export default function ImageViewer({ images, index, onClose }: { images: ViewerImage[]; index: number; onClose: () => void }) {
  const t = useT();
  const { apiPath } = usePanelConfig();
  const root = usePortalContainer();
  // The image shown (it changes as the user moves): its details are fetched if missing.
  const [shown, setShown] = useState(index);
  const current = useDetails(images[shown]);
  const slides: Slide[] = images.map((image, i) => {
    const info = i === shown && current ? current : image;
    const url = mediaUrl(apiPath, image.id);
    const details = [
      info.width !== undefined && info.height !== undefined ? `${info.width} × ${info.height}` : null,
      info.size !== undefined ? t("mediaLibrary.bytes", { n: formatCount(info.size) }) : null,
    ].filter((d): d is string => d !== null);
    return {
      src: url,
      alt: info.alt ?? info.name ?? "",
      title: info.name,
      description: [info.alt, details.join(" · ")].filter(Boolean).join("\n") || undefined,
      download: { url, filename: info.name ?? "" },
    };
  });
  return (
    <Lightbox
      open
      close={onClose}
      // The image shown: the lightbox goes back to `index` when its slides
      // change, as they do when the shown image's details arrive.
      index={shown}
      slides={slides}
      plugins={[Captions, Counter, Download, Zoom]}
      on={{ view: ({ index: i }) => setShown(i) }}
      carousel={{ finite: images.length <= 1 }}
      render={images.length <= 1 ? { buttonPrev: () => null, buttonNext: () => null } : undefined}
      controller={{ closeOnBackdropClick: true }}
      zoom={{ maxZoomPixelRatio: 4, scrollToZoom: true }}
      captions={{ descriptionMaxLines: 4, descriptionTextAlign: "start" }}
      // The counter sits bottom right: top left is the title's (the library
      // draws both there). Styles through React (the CSSOM), as the library's.
      counter={{ container: { style: { top: "unset", left: "unset", bottom: 0, right: 0 } } }}
      // Escape closes the viewer only, not a <dialog> it is drawn in (the
      // media picker): its key stops here, as a menu's does.
      portal={{ root, container: { onKeyDownCapture: (e) => { if (e.key === "Escape") e.preventDefault(); } } }}
      labels={{
        Previous: t("viewer.previous"),
        Next: t("viewer.next"),
        Close: t("action.close"),
        "Zoom in": t("viewer.zoomIn"),
        "Zoom out": t("viewer.zoomOut"),
        Download: t("viewer.download"),
        Lightbox: t("viewer.label"),
        "Photo gallery": t("viewer.images"),
        Slide: t("viewer.slide"),
        Carousel: t("viewer.images"),
        "{index} of {total}": t("viewer.counter", { index: "{index}", total: "{total}" }),
      }}
    />
  );
}
