*** Settings ***
Documentation       Verify 4.7 Geospatial Properties.
...
...                 4.7.1: the Value of a GeoProperty shall be a GeoJSON Geometry
...                 (GeometryCollection excluded); location is a GeoProperty.
...                 4.7.2: a whole geometry may be encoded as a JSON string —
...                 "Implementations shall accept the referred encoded string
...                 value, if and only if, it can be parsed into a JSON Object
...                 ... representing a valid Geometry of the type specified."
...
...                 Antares extension TP — official geo TPs (001_12/13, 010_08/09,
...                 013_05/06, 018_08/09) never send the 4.7.2 string encoding
...                 nor a non-Geo location.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
471_01_01 String-Encoded Geometry Is Accepted And Normalized
    [Documentation]    4.7.2: a GeoProperty whose value is the JSON-string
    ...    encoding of a Point is accepted; the served value is the geometry
    ...    OBJECT (not a string) with the same coordinates.
    [Tags]    e-create    e-retrieve    4_7_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": {"type": "GeoProperty", "value": "{\\"type\\": \\"Point\\", \\"coordinates\\": [17.1, 48.7]}"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${value}=    Evaluate    $response.json()['location']['value']
    ${is_str}=    Evaluate    isinstance($value, str)
    Should Not Be True    ${is_str}    value must be normalized to an object
    ${expected}=    Evaluate    {"type": "Point", "coordinates": [17.1, 48.7]}
    Should Be Equal    ${value}    ${expected}
    [Teardown]    Delete Entity    ${entity_id}

471_01_02 Unparseable String Geometry Is Rejected
    [Documentation]    4.7.2 "if and only if": a string value that does not parse
    ...    into a valid geometry → 400 BadRequestData, entity NOT created.
    [Tags]    e-create    4_7_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": {"type": "GeoProperty", "value": "not a geometry"}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

471_01_03 Location As A Plain Property Is Rejected
    [Documentation]    4.7.1: location is defined as a GeoProperty — declaring it
    ...    a plain Property → 400 BadRequestData.
    [Tags]    e-create    4_7_1    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "location": {"type": "Property", "value": 7}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
