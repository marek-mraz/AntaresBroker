package com.testrunner.service;

import java.io.*;
import java.net.URI;
import java.net.http.*;
import java.nio.file.*;
import java.util.*;
import java.util.zip.*;

import freemarker.template.TemplateException;

import org.springframework.stereotype.Service;

import com.fasterxml.jackson.databind.ObjectMapper;

@Service
public class ItbPostGenerationService {
    private final XmlGeneratorService xmlGeneratorService;
    
    public ItbPostGenerationService(
        XmlGeneratorService xmlGeneratorService
    ) {
        this.xmlGeneratorService = xmlGeneratorService;
    }

    private static final Path GENERATED_BASE_DIR = Paths.get("/opt/generated-itb-xml");

    public void completeAndDeployTestSuites() throws IOException {
        
        try (InputStream is = getClass().getClassLoader().getResourceAsStream("test-request.json")) {

            if (is == null) {
                throw new FileNotFoundException(
                    "test-request.json not found in resources");
            }
        }

        List<Path> leafDirs = Files.walk(GENERATED_BASE_DIR)
                                    .filter(Files::isDirectory)
                                    .filter(this::isLeafDirectory)
                                    .toList();

        for (Path dir : leafDirs) {
            try {
                xmlGeneratorService.generateXmlTestSuiteFile(dir);
            } catch (IOException | TemplateException e) {
                e.printStackTrace();
            }
            copyToResources(dir);
            zipFolder(dir);
            System.out.println("try to deploy the test suite : " + dir.toString());
            try {
                sendZipAsBase64(dir);
            } catch (IOException | InterruptedException e) {
                e.printStackTrace();
            }
            
        }
    }

    private boolean isLeafDirectory(Path dir) {
        // Need to be excluded because we don't want to execute the actions on these folders
        if ("resources".equals(dir.getFileName().toString())) {
            return false;
        }

        try (DirectoryStream<Path> stream = Files.newDirectoryStream(dir)) {
            for (Path path : stream) {
                if (Files.isDirectory(path)) {
                    return false;
                }
            }
        } catch (IOException e) {
            throw new UncheckedIOException(e);
        }
        return true;
    }

    private void copyToResources(Path leafDir) throws IOException {

        Path resourcesDir = leafDir.resolve("resources");
        Files.createDirectories(resourcesDir);

        Path targetFile = resourcesDir.resolve("test-request.json");

        try (InputStream copyStream = getClass()
                .getClassLoader()
                .getResourceAsStream("test-request.json")) {

                    if (copyStream == null) {
                        throw new FileNotFoundException(
                            "test-request.json not found in resources");
                    }

                    Files.copy(
                        copyStream,
                        targetFile,
                        StandardCopyOption.REPLACE_EXISTING
                    );
                }
            
    }

    private void zipFolder(Path dir) throws IOException {

        if (!Files.isDirectory(dir)) {
            throw new IllegalArgumentException("Path must be a directory");
        }

        Path zipPath = dir.getParent().resolve(dir.getFileName() + ".zip");

        try (ZipOutputStream zos = new ZipOutputStream(
            Files.newOutputStream(zipPath))) {

                Files.walk(dir)
                        .filter(path -> !Files.isDirectory(path))
                        .forEach(path -> {
                            ZipEntry zipEntry = new ZipEntry(
                                dir.relativize(path).toString()
                            );
                            
                        try (InputStream is = Files.newInputStream(path)) {

                            zos.putNextEntry(zipEntry);
                            is.transferTo(zos);
                            zos.closeEntry();

                        } catch (IOException e) {
                            throw new UncheckedIOException(e);
                        }
            });
        }
    }

    private void sendZipAsBase64(Path path) throws IOException, InterruptedException {

        Path zipPath = path.getParent().resolve(path.getFileName() + ".zip");

        byte[] fileBytes = Files.readAllBytes(zipPath);

        String base64Zip = Base64.getEncoder().encodeToString(fileBytes);
        
        String jsonBody = new ObjectMapper().writeValueAsString(Map.of(
    "specification", "BA2105E5X408DX4007XBD50X461A38F9953E",
    "testSuite", base64Zip,
    "ignoreWarnings", true
        ));

        HttpRequest request = HttpRequest.newBuilder()
                .uri(URI.create("http://itb-ui:9000/api/rest/testsuite/deploy"))
                .header("ITB_API_KEY", "266DD72DX1AB4X4CBEX91BEXA9204AC58F37")
                .header("Content-Type", "application/json")
                .POST(HttpRequest.BodyPublishers.ofString(jsonBody))
                .build();

        HttpClient client = HttpClient.newHttpClient();
        HttpResponse<String> response = client.send(
                request,
                HttpResponse.BodyHandlers.ofString()
        );

        System.out.println("Status: " + response.statusCode());
        System.out.println("Response: " + response.body());

    }
}