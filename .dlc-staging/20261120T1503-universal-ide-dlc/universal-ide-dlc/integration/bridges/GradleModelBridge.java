/* Reference JVM bridge. Compile with org.gradle:gradle-tooling-api on classpath.
 * Runs Gradle project configuration; MUST be invoked only on trusted projects.
 * This bridge emits a limited model. Expand it to support composite builds,
 * Kotlin/Android variants and fault-tolerant Gradle model fetching.
 */
import java.io.File;
import java.util.*;
import org.gradle.tooling.GradleConnector;
import org.gradle.tooling.ProjectConnection;
import org.gradle.tooling.model.GradleProject;
import org.gradle.tooling.model.GradleTask;

public final class GradleModelBridge {
    private static String quote(String s) {
        StringBuilder out = new StringBuilder("\"");
        for (char c : s.toCharArray()) {
            if (c == '"' || c == '\\') out.append('\\').append(c);
            else if (c == '\n') out.append("\\n");
            else if (c < 0x20) out.append(' ');
            else out.append(c);
        }
        return out.append('"').toString();
    }
    private static void add(GradleProject p, StringBuilder out) {
        out.append("{\"name\":").append(quote(p.getName()))
           .append(",\"path\":").append(quote(p.getPath())).append(",\"tasks\":[");
        boolean first = true;
        for (GradleTask task : p.getTasks()) {
            if (!first) out.append(','); first=false;
            out.append(quote(task.getPath()));
        }
        out.append("],\"children\":["); first=true;
        for (GradleProject child : p.getChildren()) {
            if (!first) out.append(','); first=false; add(child,out);
        }
        out.append("]}");
    }
    public static void main(String[] args) {
        if (args.length != 1) {
            System.err.println("Usage: GradleModelBridge <trusted-project-root>");
            System.exit(2);
        }
        try (ProjectConnection conn = GradleConnector.newConnector()
                .forProjectDirectory(new File(args[0])).connect()) {
            GradleProject model = conn.getModel(GradleProject.class);
            StringBuilder out = new StringBuilder(); add(model,out);
            System.out.println(out);
        }
    }
}
