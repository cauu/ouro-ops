import { getCurrentWindow } from "@tauri-apps/api/window";
import type { MouseEvent } from "react";

export function onDragRegionMouseDown(e: MouseEvent): void {
  // Only left-click; ignore if target is inside a .no-drag element.
  if (e.button !== 0) return;
  if ((e.target as HTMLElement).closest(".no-drag")) return;
  e.preventDefault();
  void getCurrentWindow().startDragging();
}
