// The image viewer: images of the media library open over the page, go
// through the place's other images, show the file's details (fetched from its
// media record when the place only has an id), and a click never changes a
// form's value.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { http, HttpResponse } from "msw";
import { setupServer } from "msw/node";
import { useState } from "react";
import { SessionContext } from "@/api/context";
import type { InputSchema } from "@/api/types";
import { createSession } from "@/app/session";
import { FileUploadWidget } from "@/renderers/media";
import { RichTextView } from "@/renderers/rich-text";
import { type ViewerImage, useImageViewer } from "@/components/lightbox";

const API = "http://localhost/admin/api";
const A = "0199d3a4-1b2c-7d3e-8f40-00000000000a";
const B = "0199d3a4-1b2c-7d3e-8f40-00000000000b";
const looked: string[] = [];

const server = setupServer(
  http.get("*/admin/api/auth/csrf", () => HttpResponse.json({ csrfToken: "t", userId: "1" })),
  http.get("*/admin/api/panel", () => HttpResponse.json({ brand: "x", user: { id: "1", name: "a", email: "a@b.c" }, navigation: [], system: {}, settings: [], pages: [], permissions: [] })),
  http.get("*/admin/api/resources/media/records/:id", ({ params }) => {
    looked.push(String(params.id));
    const name = params.id === A ? "cat.png" : "dog.png";
    return HttpResponse.json({
      data: { key: params.id, title: name, abilities: {}, data: { name, mime: "image/png", size: "2048", width: 800, height: 600, alt: params.id === A ? "A cat" : null } },
    });
  }),
);
beforeAll(() => server.listen({ onUnhandledRequest: "error" }));
afterAll(() => server.close());
beforeEach(() => {
  looked.length = 0;
});

function Providers({ children }: { children: React.ReactNode }) {
  const client = new QueryClient();
  const session = createSession({ basePath: "/admin", apiPath: API }, client, () => {});
  return (
    <SessionContext.Provider value={session}>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </SessionContext.Provider>
  );
}

const input = (media: InputSchema["media"]) =>
  ({ name: "photos", label: "Photos", type: "file_upload", valueType: "json", nullable: true, disabled: false, rules: [], columnSpan: 1, media }) as unknown as InputSchema;

const viewer = () => screen.queryByLabelText("Image viewer");

describe("image viewer", () => {
  it("opens from a form's file with its details, and closes, the value unchanged", async () => {
    const onChange = vi.fn();
    render(<Providers><FileUploadWidget id="f" input={input({ multiple: true, maxItems: 3, accept: [] })} value={[A, B]} onChange={onChange} invalid={false} /></Providers>);
    const open = screen.getAllByRole("button", { name: "View image" });
    expect(open).toHaveLength(2);
    await userEvent.click(open[0]!);
    await waitFor(() => expect(viewer()).toBeInTheDocument());
    // Details from the media record: name, alt, size, dimensions.
    expect(await screen.findByText("cat.png")).toBeInTheDocument();
    expect(screen.getByText(/A cat/)).toBeInTheDocument();
    expect(screen.getByText(/800 × 600 · 2,048 bytes/)).toBeInTheDocument();
    expect(screen.getByText("1 / 2")).toBeInTheDocument();
    // Moving between images needs a laid-out page: the E2E tests check it.
    expect(screen.getByRole("button", { name: "Next image" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Zoom in" })).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    await waitFor(() => expect(viewer()).not.toBeInTheDocument());
    expect(onChange).not.toHaveBeenCalled();
    // Only the image shown was looked up.
    expect(looked).toEqual([A]);
  });

  it("is off with preview: false", () => {
    const { container } = render(<Providers><FileUploadWidget id="f" input={input({ multiple: true, maxItems: 3, accept: [], preview: false })} value={[A]} onChange={() => {}} invalid={false} /></Providers>);
    expect(screen.queryByRole("button", { name: "View image" })).toBeNull();
    expect(container.querySelector("img")).not.toBeNull();
  });

  it("goes through every image of a document, from the one clicked", async () => {
    const doc = {
      type: "doc",
      version: 2,
      content: [
        { type: "image", attrs: { media: A, alt: "A cat" } },
        { type: "paragraph", content: [{ type: "text", text: "Between" }] },
        { type: "image", attrs: { media: B, alt: "" } },
      ],
    };
    render(<Providers><RichTextView doc={doc as never} /></Providers>);
    expect(within(screen.getByRole("button", { name: "View A cat" })).getByRole("img")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "View image" }));
    await waitFor(() => expect(viewer()).toBeInTheDocument());
    expect(screen.getByText("2 / 2")).toBeInTheDocument();
  });

  it("counts only the images a document shows, and a file that cannot be loaded opens nothing", async () => {
    const doc = {
      type: "doc",
      version: 3,
      content: [
        { type: "image", attrs: { media: A, alt: "A cat" } },
        // An application element shows none of its content.
        { type: "app.box", attrs: {}, content: [{ type: "image", attrs: { media: B, alt: "hidden" } }] },
      ],
    };
    render(<Providers><RichTextView doc={doc as never} /></Providers>);
    const cat = screen.getByRole("button", { name: "View A cat" });
    // Not loaded: the "unavailable" chip, no button.
    fireEvent.error(within(cat).getByRole("img"));
    expect(screen.queryByRole("button", { name: "View A cat" })).toBeNull();
    expect(screen.getByRole("img", { name: "File unavailable" })).toBeInTheDocument();
  });

  it("closes by itself when the place's images change", async () => {
    let set: (images: ViewerImage[]) => void = () => {};
    function Place() {
      const [images, setImages] = useState<ViewerImage[]>([{ id: A, name: "a.png", size: 1 }, { id: B, name: "b.png", size: 1 }]);
      set = setImages;
      const viewer = useImageViewer(images);
      return <><button type="button" onClick={() => viewer.open(1)}>open</button>{viewer.viewer}</>;
    }
    render(<Providers><Place /></Providers>);
    await userEvent.click(screen.getByRole("button", { name: "open" }));
    await waitFor(() => expect(viewer()).toBeInTheDocument());
    // Another page: the viewer does not show its second image, nor open again later.
    act(() => set([{ id: B, name: "b.png", size: 1 }]));
    await waitFor(() => expect(viewer()).not.toBeInTheDocument());
    act(() => set([{ id: A, name: "a.png", size: 1 }, { id: B, name: "b.png", size: 1 }]));
    expect(viewer()).not.toBeInTheDocument();
  });
});
