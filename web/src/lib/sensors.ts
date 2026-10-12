import { MouseSensor, type SensorDescriptor, type SensorOptions, TouchSensor, useSensor, useSensors } from "@dnd-kit/core";

/** Mouse and pen start a drag after this many px of movement. */
export const MOUSE_DRAG_DISTANCE = 4;
/** Touch starts a drag after this long hold (ms); before it the browser scrolls natively. */
export const TOUCH_HOLD_DELAY = 250;
/** Touch movement (px) allowed during the hold; beyond it the touch is a scroll. */
export const TOUCH_HOLD_TOLERANCE = 5;

/** Drag activation shared by the timeline and the task board: mouse by distance, touch by long-press. */
export function useTouchAwareSensors(extra: SensorDescriptor<SensorOptions>[] = []) {
  return useSensors(
    useSensor(MouseSensor, { activationConstraint: { distance: MOUSE_DRAG_DISTANCE } }),
    useSensor(TouchSensor, { activationConstraint: { delay: TOUCH_HOLD_DELAY, tolerance: TOUCH_HOLD_TOLERANCE } }),
    ...extra,
  );
}
