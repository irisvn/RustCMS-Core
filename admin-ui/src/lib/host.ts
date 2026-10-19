// The extension host API v2 (ADR 0032): React pages, slots and cells that
// render inside the panel's tree, on the panel's React, React Query and UI kit.
// It sits beside v1 (`widgets.ts`), which stays unchanged.

import * as React from "react";
import * as jsxRuntime from "react/jsx-runtime";
import { createPortal } from "react-dom";
import { useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError } from "@/api/client";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox, Switch } from "@/components/ui/checkbox";
import { Dialog } from "@/components/ui/dialog";
import { Input, Label, NativeSelect, Textarea } from "@/components/ui/input";
import { DropdownMenu, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, Tooltip } from "@/components/ui/menu";
import { CodeBlock, Separator, Skeleton, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/misc";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Page, PageHeader } from "@/components/layout";
import { ErrorState, Loading } from "@/components/states";
import { cn } from "@/lib/utils";

/**
 * The host's additive level (ADR 0032): the `ui` module's exports and the
 * props of v2 views. A bundle needs a level at most this one; removing
 * anything takes host API 3.
 */
export const UI_VERSION = 2;

/** The panel's versions, for `requires`. */
export const HOST_VERSIONS = Object.freeze({
  api: 2,
  react: React.version,
  reactQuery: __RUSTCMS_HOST__.reactQuery,
  ui: UI_VERSION,
});

/** What a v2 script needs (from `AdminProvider::extension_script_v2`). */
export interface Requires {
  api: number;
  react: string;
  reactQuery: string;
  ui: number;
}

const majorMinor = (version: string): [number, number] | null => {
  const match = /^(\d+)\.(\d+)/.exec(version);
  return match ? [Number(match[1]), Number(match[2])] : null;
};

/** Why `requires` does not fit this panel, or `null` when it does. */
export function incompatibility(requires: Requires, host = HOST_VERSIONS): string | null {
  if (requires.api !== host.api) return `needs host API ${requires.api}, the panel has ${host.api}`;
  if (!Number.isInteger(requires.ui) || requires.ui < 1 || requires.ui > host.ui) return `needs ui ${requires.ui}, the panel has ${host.ui}`;
  for (const [name, wanted, have] of [
    ["React", requires.react, host.react],
    ["React Query", requires.reactQuery, host.reactQuery],
  ] as const) {
    const w = majorMinor(wanted);
    const h = majorMinor(have);
    if (!w || !h || w[0] !== h[0] || h[1] < w[1]) return `needs ${name} ${wanted}, the panel has ${have}`;
  }
  return null;
}

const freeze = <T extends object>(value: T): Readonly<T> => Object.freeze(value);

/** The host's own instances, by the import id the SDK's preset maps. */
export const MODULES = freeze({
  react: React,
  "react/jsx-runtime": freeze({ jsx: jsxRuntime.jsx, jsxs: jsxRuntime.jsxs, Fragment: jsxRuntime.Fragment }),
  "react-dom": freeze({ createPortal }),
  "@tanstack/react-query": freeze({ useQuery, useMutation, useQueryClient, useInfiniteQuery }),
  ui: freeze({
    Alert,
    Badge,
    Button,
    buttonVariants,
    Card,
    CardContent,
    CardHeader,
    CardTitle,
    Checkbox,
    Dialog,
    Input,
    Label,
    NativeSelect,
    Switch,
    Textarea,
    Page,
    PageHeader,
    Loading,
    ErrorState,
    cn,
    // ui 2
    Tabs,
    TabsList,
    TabsTrigger,
    TabsContent,
    DropdownMenu,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuLabel,
    Tooltip,
    Table,
    TableHeader,
    TableBody,
    TableRow,
    TableHead,
    TableCell,
    Skeleton,
    Separator,
    CodeBlock,
  }),
  host: freeze({ ApiError, versions: HOST_VERSIONS }),
});
