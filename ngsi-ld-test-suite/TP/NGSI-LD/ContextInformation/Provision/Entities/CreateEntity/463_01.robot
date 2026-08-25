*** Settings ***
Documentation       Verify 4.6.3 Supported data types for Values.
...
...                 4.6.3: Values support "All the GeoJSON Geometries [8] with the
...                 exception of GeometryCollection"; DateTime strings use
...                 YYYY-MM-DDThh:mm:ssZ — "The string shall not contain
...                 expressions of the difference between local time and UTC"
...                 (offsets invalid, trailing Z mandatory).
...
...                 Antares extension TP — the official suite never sends a
...                 GeometryCollection value nor an offset timestamp.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
463_01_01 GeometryCollection Value Is Rejected
    [Documentation]    4.6.3: GeoJSON geometries are supported "with the exception
    ...    of GeometryCollection" → 400 BadRequestData, and the entity must NOT
    ...    have been created.
    [Tags]    e-create    4_6_3    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": {"type": "GeoProperty", "value": {"type": "GeometryCollection", "geometries": [{"type": "Point", "coordinates": [17.1, 48.7]}]}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

463_01_02 Point Geometry Round-Trips
    [Documentation]    4.6.3: a plain GeoJSON Point is a supported Value — 201 and
    ...    served back unchanged (still a Point, no GeometryCollection wrapper).
    [Tags]    e-create    e-retrieve    4_6_3    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [17.1, 48.7]}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${gtype}=    Evaluate    $response.json()['location']['value']['type']
    Should Be Equal    ${gtype}    Point
    Should Not Contain    ${response.text}    GeometryCollection
    [Teardown]    Delete Entity    ${entity_id}

463_01_03 ObservedAt With A UTC Offset Is Rejected
    [Documentation]    4.6.3: "The string shall not contain expressions of the
    ...    difference between local time and UTC" — +02:00 offset → 400
    ...    BadRequestData.
    [Tags]    e-create    4_6_3    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "speed": {"type": "Property", "value": 3, "observedAt": "2026-08-10T12:00:00+02:00"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
