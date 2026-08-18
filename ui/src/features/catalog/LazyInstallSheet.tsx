import { lazy, Suspense, type ComponentProps } from "react";

type InstallSheetModule = typeof import("./InstallSheet");

const InstallSheet = lazy(() =>
  import("./InstallSheet").then((module) => ({ default: module.InstallSheet })),
);

export type InstallSheetProps = ComponentProps<InstallSheetModule["InstallSheet"]>;

/** The Install sheet only exists while open, so its code loads on first use, not at startup. */
export function LazyInstallSheet(props: InstallSheetProps) {
  return (
    <Suspense fallback={null}>
      <InstallSheet {...props} />
    </Suspense>
  );
}
