def to_csv(tasks):
    lines = ["id,title,tags,status,due"]
    for task in tasks:
        due = task.due.isoformat() if task.due else ""
        lines.append(f"{task.id},{task.title},{';'.join(task.tags)},{task.status},{due}")
    return "\n".join(lines) + "\n"
