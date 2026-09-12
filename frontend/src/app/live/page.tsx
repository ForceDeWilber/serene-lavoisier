import { redirect } from "next/navigation";

export default function LiveRoute() {
  redirect("/dashboard?mode=live");
}
