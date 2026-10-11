def search(tasks, query):
    return [task for task in tasks if query in task.title]
