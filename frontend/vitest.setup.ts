import "@testing-library/jest-dom/vitest";
import { afterEach } from "vitest";
import { cleanup } from "@testing-library/react";

// vitest.config.ts does not set test.globals, so React Testing Library's automatic
// afterEach cleanup (which relies on a global `afterEach`) never registers. Without
// this, multiple render() calls within one test file leave prior DOM trees mounted,
// breaking any test file with more than one test (first hit in Task 4).
afterEach(() => {
  cleanup();
});

// MUI X Charts measures its container via ResizeObserver, which jsdom does not implement.
// Without this stub, any test rendering a chart (Tasks 9-11) throws "ResizeObserver is not defined".
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
// eslint-disable-next-line @typescript-eslint/no-explicit-any
(globalThis as any).ResizeObserver ??= ResizeObserverStub;

// jsdom does not implement window.matchMedia, which ThemeRegistry (Task 4) uses to
// detect the OS color-scheme preference. Without this stub, rendering ThemeRegistry
// throws "window.matchMedia is not a function".
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    }) as MediaQueryList;
}
