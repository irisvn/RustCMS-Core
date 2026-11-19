// The levels a repeater row's conditions read outside the row (ADR 0044):
// the record's form, set by the form's outermost components, and the
// enclosing row, set by each row for the repeaters nested in it.
import { createContext, useContext } from "react";
import type { Outer } from "@/schema/conditions";

export const OuterContext = createContext<Outer>({});

/** The levels around the rows rendered here. */
export const useOuter = (): Outer => useContext(OuterContext);
