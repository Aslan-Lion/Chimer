import { DisplayWindow } from "../components/display/DisplayWindow";

/**
 * Interface mounted inside the native presentation window.
 *
 * The label of the native window decides which interface runs — both windows
 * share one bundle — so this component is intentionally nothing but the
 * presentation surface.
 */
export function DisplayApp() {
  return <DisplayWindow />;
}
