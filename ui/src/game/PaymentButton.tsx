import { useEffect, useRef, type ReactNode } from "react";
import { Button } from "../components/Controls";

/** A short tap accepts. Holding a touch only previews, even when released on the button. */
export function PaymentButton({
  children,
  disabled,
  variant,
  onPreview,
  onPay,
}: {
  children: ReactNode;
  disabled?: boolean;
  variant?: "primary" | "secondary";
  onPreview(on: boolean): void;
  onPay(): void;
}) {
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const held = useRef(false);
  useEffect(() => () => clearTimeout(timer.current), []);
  const end = () => {
    clearTimeout(timer.current);
    onPreview(false);
  };
  return (
    <Button
      disabled={disabled}
      variant={variant}
      onPointerEnter={(e) => {
        if (e.pointerType === "mouse" && !disabled) onPreview(true);
      }}
      onPointerLeave={end}
      onPointerDown={(e) => {
        held.current = false;
        if (e.pointerType === "mouse") return;
        timer.current = setTimeout(() => {
          held.current = true;
          onPreview(true);
        }, 450);
      }}
      onPointerUp={(e) => {
        if (e.pointerType !== "mouse") end();
      }}
      onPointerCancel={end}
      onContextMenu={(e) => e.preventDefault()}
      onClick={(e) => {
        if (!held.current || e.detail === 0) {
          onPreview(false);
          onPay();
        }
        held.current = false;
      }}
    >
      {children}
    </Button>
  );
}
