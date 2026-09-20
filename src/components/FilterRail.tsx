import { CaretLeft, CaretRight } from "@phosphor-icons/react";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

type FilterRailProps<T extends string> = {
  label: string;
  options: readonly T[];
  value: T;
  onChange: (value: T) => void;
  renderLeading?: (option: T) => ReactNode;
  renderTrailing?: (option: T) => ReactNode;
};

export function FilterRail<T extends string>({
  label,
  options,
  value,
  onChange,
  renderLeading,
  renderTrailing,
}: FilterRailProps<T>) {
  const railRef = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState(false);
  const [canScrollBack, setCanScrollBack] = useState(false);
  const [canScrollForward, setCanScrollForward] = useState(false);

  const measureOverflow = useCallback(() => {
    const rail = railRef.current;
    if (!rail) return;

    const hasOverflow = rail.scrollWidth > rail.clientWidth + 1;
    setOverflow(hasOverflow);
    setCanScrollBack(hasOverflow && rail.scrollLeft > 1);
    setCanScrollForward(
      hasOverflow && rail.scrollLeft + rail.clientWidth < rail.scrollWidth - 1,
    );
  }, []);

  useEffect(() => {
    const rail = railRef.current;
    if (!rail) return;

    measureOverflow();
    const observer = new ResizeObserver(measureOverflow);
    observer.observe(rail);
    Array.from(rail.children).forEach((child) => observer.observe(child));
    rail.addEventListener("scroll", measureOverflow, { passive: true });

    return () => {
      observer.disconnect();
      rail.removeEventListener("scroll", measureOverflow);
    };
  }, [measureOverflow, options]);

  const scroll = (direction: -1 | 1) => {
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    railRef.current?.scrollBy({
      left: direction * 148,
      behavior: reduceMotion ? "auto" : "smooth",
    });
  };

  return (
    <div className="filter-rail" data-overflow={overflow} aria-label={label}>
      {overflow && (
        <button
          className="rail-arrow"
          type="button"
          aria-label={`Scroll ${label.toLowerCase()} left`}
          title="Scroll left"
          disabled={!canScrollBack}
          onClick={() => scroll(-1)}
        >
          <CaretLeft aria-hidden="true" weight="bold" />
        </button>
      )}

      <div className="filter-track" ref={railRef} role="group" aria-label={label}>
        {options.map((option) => {
          const selected = option === value;
          return (
            <button
              key={option}
              type="button"
              className="filter-chip"
              aria-pressed={selected}
              onClick={() => onChange(option)}
            >
              {renderLeading?.(option)}
              <span className="filter-chip-label">{option}</span>
              {renderTrailing?.(option)}
            </button>
          );
        })}
      </div>

      {overflow && (
        <button
          className="rail-arrow"
          type="button"
          aria-label={`Scroll ${label.toLowerCase()} right`}
          title="Scroll right"
          disabled={!canScrollForward}
          onClick={() => scroll(1)}
        >
          <CaretRight aria-hidden="true" weight="bold" />
        </button>
      )}
    </div>
  );
}
