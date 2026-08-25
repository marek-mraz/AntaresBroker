*** Settings ***
Documentation       Verify the 4.5.16 GeoJSON representation of entities.
...
...                 4.5.16.1: default GeoProperty is "location"; with multiple
...                 instances the default (no datasetId) instance is selected; a
...                 missing GeoProperty or non-geometry value yields geometry null.
...                 4.5.16.2: Feature = id + fixed type "Feature" + geometry +
...                 properties (entity type and attributes, no id).
...                 4.5.16.3: FeatureCollection = fixed type + features array,
...                 no per-Feature @context.
...
...                 Antares extension TP — official 019_27 covers only the
...                 geometryProperty parameter validation.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
4516_01_01 Feature Shape With Default Instance Geometry
    [Documentation]    4.5.16.1/4.5.16.2: default (no-datasetId) location instance
    ...    becomes the geometry; properties carry type + attributes but never id.
    [Tags]    e-retrieve    4_5_16_1    4_5_16_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [9.0, 9.0]}, "datasetId": "urn:ngsi-ld:Dataset:gps"}, {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}], "speed": {"type": "Property", "value": 5}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=application/geo+json
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${ftype}=    Evaluate    $body['type']
    Should Be Equal    ${ftype}    Feature
    ${fid}=    Evaluate    $body['id']
    Should Be Equal    ${fid}    ${entity_id}
    ${geom}=    Evaluate    $body['geometry']
    ${expected_geom}=    Evaluate    {"type": "Point", "coordinates": [1.0, 2.0]}
    Should Be Equal    ${geom}    ${expected_geom}
    ${props_type}=    Evaluate    $body['properties']['type']
    Should Be Equal    ${props_type}    Vehicle
    ${id_leak}=    Evaluate    'id' in $body['properties']
    Should Not Be True    ${id_leak}
    ${speed_present}=    Evaluate    'speed' in $body['properties']
    Should Be True    ${speed_present}

4516_01_02 Non-Geometry Or Missing GeoProperty Yields Null Geometry
    [Documentation]    4.5.16.1: "If an entity lacks the GeoProperty as specified
    ...    or the value does not hold a valid GeoJSON geometry object then the
    ...    geometry shall be undefined and returned with a value of null".
    [Tags]    e-retrieve    4_5_16_1    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 5}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=application/geo+json
    ...    geometryProperty=speed
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${geom}=    Evaluate    $response.json()['geometry']
    Should Be Equal    ${geom}    ${None}
    ${response}=    Retrieve Entity
    ...    ${entity_id}
    ...    accept=application/geo+json
    ...    context=${ngsild_test_suite_context}
    ${geom}=    Evaluate    $response.json()['geometry']
    Should Be Equal    ${geom}    ${None}

4516_01_03 FeatureCollection For Query
    [Documentation]    4.5.16.3: query responses are a FeatureCollection whose
    ...    features are 4.5.16.2 Feature objects without per-Feature @context.
    [Tags]    e-query    4_5_16_3    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Test Variable    ${entity_id}
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${context_link}=    Build Context Link    ${ngsild_test_suite_context}
    &{headers}=    Create Dictionary    Accept=application/geo+json    Link=${context_link}
    &{params}=    Create Dictionary    type=Vehicle    id=${entity_id}
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    params=${params}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    ${body}=    Set Variable    ${response.json()}
    ${ctype}=    Evaluate    $body['type']
    Should Be Equal    ${ctype}    FeatureCollection
    ${features}=    Evaluate    $body['features']
    Length Should Be    ${features}    1
    ${f_type}=    Evaluate    $features[0]['type']
    Should Be Equal    ${f_type}    Feature
    ${ctx_leak}=    Evaluate    '@context' in $features[0]
    Should Not Be True    ${ctx_leak}
