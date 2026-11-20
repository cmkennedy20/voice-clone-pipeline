import '@testing-library/jest-dom/vitest';

if (typeof URL.createObjectURL === 'undefined') {
  let blobCounter = 0;
  URL.createObjectURL = () => `blob:mock-audio-url-${++blobCounter}`;
}

if (typeof URL.revokeObjectURL === 'undefined') {
  URL.revokeObjectURL = () => {};
}

if (typeof HTMLMediaElement !== 'undefined') {
  HTMLMediaElement.prototype.play = () => Promise.resolve();
  HTMLMediaElement.prototype.pause = () => {};
  HTMLMediaElement.prototype.load = () => {};
}
