import { render, screen } from "@testing-library/react";
import { App } from "./App";

// Phase 1 scaffold smoke test: proves the TS test runner, JSX pipeline, and
// DOM environment all work before any real screen depends on them.
describe("App shell", () => {
  it("renders", () => {
    render(<App />);
    expect(screen.getByRole("heading", { name: "MYDB" })).toBeInTheDocument();
  });
});
