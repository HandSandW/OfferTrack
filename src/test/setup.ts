import "@testing-library/jest-dom/vitest";

// jsdom does not implement the native dialog top layer. The app always uses
// showModal in WebView2; tests expose the same open/closed visibility contract.
HTMLDialogElement.prototype.showModal = function () {
  this.open = true;
};
HTMLDialogElement.prototype.close = function () {
  this.open = false;
};
