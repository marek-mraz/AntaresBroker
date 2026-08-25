*** Settings ***
Documentation       Verify 5.5.5 Default @context assignment.
...
...                 5.5.5: "If the input provided by an API client does not
...                 include any @context, then the implementation shall at
...                 minimum assign the Core @context to such an input."
...
...                 Antares extension TP — exercises the whole loop: create
...                 with no @context (no Link, application/json), retrieve
...                 with no @context, and check the served context is the
...                 Core @context with terms compacted accordingly.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
555_01_01 No-Context Input Is Assigned The Core Context
    [Documentation]    5.5.5: a create without any @context succeeds under the
    ...    Core @context; a retrieval without any @context serves the Core
    ...    @context and the term compacted under the default vocabulary — the
    ...    expanded default-vocab IRI must NOT leak into the response.
    [Tags]    e-create    e-retrieve    5_5_5    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "temperature": {"type": "Property", "value": 21}}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Contain    ${response.json()['@context']}    ngsi-ld-core-context
    Should Be Equal As Integers    ${response.json()['temperature']['value']}    21
    Should Not Contain    ${response.text}    default-context/temperature
    [Teardown]    Delete Entity    ${entity_id}

555_01_02 Core Terms Resolve Without Any User Context
    [Documentation]    5.5.5: the assigned Core @context maps core terms —
    ...    a GeoProperty named location round-trips as a typed GeoProperty,
    ...    not as a default-vocab Property.
    [Tags]    e-create    e-retrieve    5_5_5    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    &{headers}=    Create Dictionary    Content-Type=application/json
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [17.6, 48.7]}}}
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    json=${payload}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    201    ${response.status_code}
    &{headers}=    Create Dictionary    Accept=application/ld+json
    ${response}=    GET
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['location']['type']}    GeoProperty
    Should Not Contain    ${response.text}    default-context/location
    [Teardown]    Delete Entity    ${entity_id}
