*** Settings ***
Documentation       Verify the 4.5.21/4.5.22 List representations.
...
...                 4.5.22.2: the normalized objectList is "an ordered array of
...                 Relationship Objects each consisting of a JSON object containing
...                 a single Attribute with a key called object". 4.5.22.3: concise
...                 input may also use bare URI strings. 4.5.21.2/3: valueList is an
...                 ordered array of Property Values; "value" shall never be present.
...
...                 Antares extension TP — the official suite contains zero
...                 ListProperty/ListRelationship coverage.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
4522_01_01 ObjectList Accepts Both Entry Forms And Normalizes On Output
    [Documentation]    4.5.22.2/4.5.22.3: bare URIs and {"object": URI} objects both
    ...    accepted on input; normalized retrieval returns {"object": URI} entries.
    [Tags]    e-create    e-retrieve    4_5_22_2    4_5_22_3    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "route": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Road:1", {"object": "urn:ngsi-ld:Road:2"}]}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${route}=    Evaluate    $response.json()['route']
    ${rtype}=    Evaluate    $route['type']
    Should Be Equal    ${rtype}    ListRelationship
    ${olist}=    Evaluate    $route['objectList']
    ${expected}=    Evaluate    [{"object": "urn:ngsi-ld:Road:1"}, {"object": "urn:ngsi-ld:Road:2"}]
    Should Be Equal    ${olist}    ${expected}
    ${bare_leak}=    Evaluate    [e for e in $olist if not isinstance(e, dict)]
    Should Be Empty    ${bare_leak}
    [Teardown]    Delete Entity    ${entity_id}

4522_01_02 Non-URI ObjectList Entry Is Rejected
    [Documentation]    4.5.22: objectList entries are Relationship objects (URIs) —
    ...    anything else is invalid content (400 BadRequestData).
    [Tags]    e-create    4_5_22_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "route": {"type": "ListRelationship", "objectList": ["not a uri"]}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}

4522_01_03 ListProperty With A Value Member Is Rejected
    [Documentation]    4.5.21.2: "value ... shall never be present, as value is a
    ...    generalization of valueList" → 400 BadRequestData.
    [Tags]    e-create    4_5_21_2    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "tyreTreadDepths": {"type": "ListProperty", "valueList": [1.2], "value": 3}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
