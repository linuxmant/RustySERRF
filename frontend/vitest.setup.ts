import "@testing-library/jest-dom/vitest";

// MUI X Charts measures its container via ResizeObserver, which jsdom does not implement.
// Without this stub, any test rendering a chart (Tasks 9-11) throws "ResizeObserver is not defined".
class ResizeObserverStub {
  observe() {}
  unobserve() {}
  disconnect() {}
}
// eslint-disable-next-line @typescript-eslint/no-explicit-any
(globalThis as any).ResizeObserver ??= ResizeObserverStub;
