import { useEffect, useId, useRef, type ReactNode } from "react";

export function Modal({
  title,
  children,
  onCancel,
  className = "",
  focusTitle = false,
}: {
  title: string;
  children: ReactNode;
  onCancel: () => void;
  className?: string;
  focusTitle?: boolean;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    if (focusTitle)
      dialog?.querySelector<HTMLElement>("h2")?.focus({ preventScroll: true });
    return () => dialog?.close();
  }, [focusTitle]);
  return (
    <dialog
      ref={ref}
      className={`modal native-modal ${className}`}
      aria-labelledby={titleId}
      onCancel={(event) => {
        event.preventDefault();
        onCancel();
      }}
    >
      <h2 id={titleId} tabIndex={focusTitle ? -1 : undefined}>
        {title}
      </h2>
      {children}
    </dialog>
  );
}
