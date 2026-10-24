// The dialog's close button (0.7.1): after the content, labelled, blocked while busy.
import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Profiler } from "react";
import { Dialog } from "@/components/ui/dialog";

describe("Dialog", () => {
  it("is named by its title and closes with ×, placed after the content", async () => {
    const onClose = vi.fn();
    render(
      <Dialog open onClose={onClose} title="Delete post?">
        <input aria-label="Reason" />
      </Dialog>,
    );
    const dialog = screen.getByRole("dialog", { name: "Delete post?" });
    const close = screen.getByRole("button", { name: "Close" });
    // After the field in the DOM: opening focuses the field first.
    expect(screen.getByLabelText("Reason").compareDocumentPosition(close) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(dialog).toContainElement(close);
    await userEvent.click(close);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape closes it unless closing is disabled", () => {
    const onClose = vi.fn();
    const { rerender } = render(<Dialog open onClose={onClose} title="T">x</Dialog>);
    fireEvent(screen.getByRole("dialog"), new Event("cancel", { cancelable: true }));
    expect(onClose).toHaveBeenCalledTimes(1);
    rerender(<Dialog open onClose={onClose} title="T" closeDisabled>x</Dialog>);
    fireEvent(screen.getByRole("dialog"), new Event("cancel", { cancelable: true }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Close" })).toBeDisabled();
  });

  it("can leave the button out", () => {
    render(<Dialog open onClose={() => {}} title="Menu" closeButton={false}>x</Dialog>);
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
  });

  it("renders once per parent render, open or closed (its ref is stable)", () => {
    // A new ref callback each render was detached and attached on every
    // commit, setting state each time: a closed dialog on a busy form
    // rendered until React gave up (error #185).
    let commits = 0;
    const tree = (n: number, open: boolean) => (
      <Profiler id="dialog" onRender={() => { commits += 1; }}>
        <Dialog open={open} onClose={() => {}} title={`T${n}`}>x</Dialog>
      </Profiler>
    );
    const { rerender } = render(tree(0, false));
    commits = 0;
    for (let n = 1; n <= 5; n++) rerender(tree(n, n % 2 === 0));
    expect(commits).toBe(5);
  });
});
