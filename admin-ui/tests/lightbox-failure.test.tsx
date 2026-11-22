// The image viewer's chunk failing to load (a deploy replaced it, the network
// dropped) closes the viewer; the page under it, and what the user typed
// there, stays.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { type ViewerImage, useImageViewer } from "@/components/lightbox";

vi.mock("@/components/image-viewer", () => {
  throw new Error("Failed to fetch dynamically imported module");
});

const IMAGES: ViewerImage[] = [{ id: "0199d3a4-1b2c-7d3e-8f40-00000000000a", name: "cat.png" }];

function Page() {
  const [draft, setDraft] = useState("");
  const { open, viewer } = useImageViewer(IMAGES);
  return (
    <div>
      <input aria-label="Title" value={draft} onChange={(e) => setDraft(e.target.value)} />
      <button type="button" onClick={() => open(0)}>
        View image
      </button>
      {viewer}
    </div>
  );
}

describe("image viewer that cannot load", () => {
  it("closes and keeps the page and its draft", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<Page />);
    fireEvent.change(screen.getByLabelText("Title"), { target: { value: "Unsaved" } });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "View image" }));
      await new Promise((resolve) => setTimeout(resolve, 20));
    });

    expect(screen.getByLabelText("Title")).toHaveValue("Unsaved");
    expect(screen.getByRole("button", { name: "View image" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Image viewer")).toBeNull();
    expect(warn).toHaveBeenCalledWith("rustcms: the image viewer could not be opened", expect.anything());
    warn.mockRestore();
    error.mockRestore();
  });
});
