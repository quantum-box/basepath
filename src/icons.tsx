import {
  AirplaneTilt,
  ArrowRight,
  ArrowsOut,
  Bell,
  BookOpen,
  CalendarBlank,
  CaretDown,
  CaretLeft,
  CaretRight,
  ChartLineUp,
  Check,
  CheckSquare,
  CircleNotch,
  Cube,
  DotsThree,
  Flag,
  Folder,
  GearSix,
  GraduationCap,
  House,
  Leaf,
  Lightbulb,
  Link,
  List,
  MagnifyingGlass,
  Minus,
  Mountains,
  Note,
  Plus,
  Quotes,
  RocketLaunch,
  ShieldCheck,
  Sparkle,
  Stack,
  Target,
  TreeStructure,
  UserCircle,
  Users,
  X,
  type Icon as PhosphorIcon,
} from "@phosphor-icons/react";
const icons: Record<string, PhosphorIcon> = {
  airplane: AirplaneTilt,
  arrow: ArrowRight,
  expand: ArrowsOut,
  bell: Bell,
  book: BookOpen,
  calendar: CalendarBlank,
  down: CaretDown,
  left: CaretLeft,
  right: CaretRight,
  chart: ChartLineUp,
  check: Check,
  tasks: CheckSquare,
  repeat: CircleNotch,
  cube: Cube,
  more: DotsThree,
  flag: Flag,
  folder: Folder,
  settings: GearSix,
  graduation: GraduationCap,
  home: House,
  leaf: Leaf,
  bulb: Lightbulb,
  link: Link,
  menu: List,
  search: MagnifyingGlass,
  minus: Minus,
  mountains: Mountains,
  note: Note,
  plus: Plus,
  quote: Quotes,
  rocket: RocketLaunch,
  shield: ShieldCheck,
  sparkle: Sparkle,
  stack: Stack,
  target: Target,
  tree: TreeStructure,
  user: UserCircle,
  users: Users,
  close: X,
};
export function Icon({
  name,
  size = 20,
  className = "",
  weight = "regular",
}: {
  name: string;
  size?: number;
  className?: string;
  weight?: "regular" | "fill" | "duotone" | "bold";
}) {
  const Component = icons[name] ?? Target;
  return (
    <Component
      size={size}
      weight={weight}
      className={className}
      aria-hidden="true"
    />
  );
}
