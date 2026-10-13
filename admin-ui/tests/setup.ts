import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(() => cleanup());

// jsdom has no scrolling (router scroll restoration).
window.scrollTo = () => {};

// jsdom has no CSS media-query engine; browser tests cover breakpoint changes.
window.matchMedia = (query) => ({
  matches: false,
  media: query,
  onchange: null,
  addListener: () => {},
  removeListener: () => {},
  addEventListener: () => {},
  removeEventListener: () => {},
  dispatchEvent: () => true,
});

// jsdom has no layout: ProseMirror (rich text editor) asks for rectangles.
const rect = { x: 0, y: 0, top: 0, left: 0, bottom: 0, right: 0, width: 0, height: 0, toJSON: () => ({}) };
const rects = () => Object.assign([rect], { item: () => rect }) as unknown as DOMRectList;
Range.prototype.getClientRects = rects;
Range.prototype.getBoundingClientRect = () => rect as DOMRect;
Element.prototype.getClientRects = rects;
document.elementFromPoint = () => null;
