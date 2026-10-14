import { Box, FileText, Folder, History, Image, LayoutList, Settings, ShieldCheck, ShoppingCart, Tag, Users } from "lucide-react";

// Icon names a resource may use (`ResourceMeta::icon`); anything else falls back.
const ICONS = {
  box: Box,
  "file-text": FileText,
  history: History,
  folder: Folder,
  image: Image,
  settings: Settings,
  shield: ShieldCheck,
  "shopping-cart": ShoppingCart,
  tag: Tag,
  users: Users,
} as const;

export function ResourceIcon({ name }: { name?: string }) {
  const Icon = (name && ICONS[name as keyof typeof ICONS]) || LayoutList;
  return <Icon className="size-4 text-muted-foreground" aria-hidden />;
}
