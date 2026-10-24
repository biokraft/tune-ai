import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { Segmented } from "./Segmented";

const options = [
  { value: "soft", label: "Soft" },
  { value: "hard", label: "Hard" },
  { value: "overdrive", label: "Overdrive" },
];

function setup(extra = {}) {
  const onChange = vi.fn();
  render(
    <Segmented
      label="Distortion mode"
      options={options}
      value="soft"
      onChange={onChange}
      {...extra}
    />,
  );
  return { onChange, group: screen.getByRole("radiogroup", { name: "Distortion mode" }) };
}

describe("Segmented", () => {
  it("renders a radiogroup with the current option checked and roving tabindex", () => {
    setup();
    const soft = screen.getByRole("radio", { name: "Soft" });
    const hard = screen.getByRole("radio", { name: "Hard" });
    expect(soft).toHaveAttribute("aria-checked", "true");
    expect(hard).toHaveAttribute("aria-checked", "false");
    expect(soft).toHaveAttribute("tabindex", "0");
    expect(hard).toHaveAttribute("tabindex", "-1");
  });

  it("ArrowRight selects the next option and moves focus", () => {
    const { onChange, group } = setup();
    fireEvent.keyDown(group, { key: "ArrowRight" });
    expect(onChange).toHaveBeenCalledWith("hard");
    expect(screen.getByRole("radio", { name: "Hard" })).toHaveFocus();
  });

  it("ArrowLeft wraps to the last option", () => {
    const { onChange, group } = setup();
    fireEvent.keyDown(group, { key: "ArrowLeft" });
    expect(onChange).toHaveBeenCalledWith("overdrive");
  });

  it("clicking another option selects it, clicking the current one does nothing", () => {
    const { onChange } = setup();
    fireEvent.click(screen.getByRole("radio", { name: "Soft" }));
    expect(onChange).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("radio", { name: "Overdrive" }));
    expect(onChange).toHaveBeenCalledWith("overdrive");
  });

  it("blocks clicks and keys when disabled", () => {
    const { onChange, group } = setup({ disabled: true });
    fireEvent.click(screen.getByRole("radio", { name: "Hard" }));
    fireEvent.keyDown(group, { key: "ArrowRight" });
    expect(onChange).not.toHaveBeenCalled();
  });
});
