*** Settings ***
Documentation       Verify 5.5.4 General NGSI-LD validation.
...
...                 5.5.4: a payload that is not valid JSON is InvalidRequest.
...                 "urn:ngsi-ld:null" as a first-level member value, or as the
...                 value of a key-value pair within a JSON object that is the
...                 right-hand side of a Property value, "shall result in an
...                 error of type BadRequestData" — the object-nested form is
...                 excepted only for NGSI-LD Fragments used in merge
...                 operations (5.5.12), where the merge goes "into JSON
...                 objects representing a Property value" (RFC 7396).
...
...                 Antares extension TP — official TPs never place the null
...                 sentinel at these positions.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
554_01_01 First-Level Null Member Value Rejected On Create
    [Documentation]    5.5.4: "urn:ngsi-ld:null" as a first level member value
    ...    → 400 BadRequestData; the entity must NOT exist afterwards.
    [Tags]    e-create    5_5_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "scope": "urn:ngsi-ld:null"}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}
    # the null URN is not creatable as an id either
    ${payload}=    Evaluate
    ...    {"id": "urn:ngsi-ld:null", "type": "Vehicle", "@context": [$ngsild_test_suite_context]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}

554_01_02 Null Inside Compound Property Value Rejected On Create
    [Documentation]    5.5.4: "urn:ngsi-ld:null" as the value of a key value
    ...    pair within a JSON object which is the Property value → 400
    ...    BadRequestData on create, at any depth.
    [Tags]    e-create    5_5_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "address": {"type": "Property", "value": {"country": "urn:ngsi-ld:null"}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    # deeper nesting is caught too
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "address": {"type": "Property", "value": {"geo": {"country": "urn:ngsi-ld:null"}}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    404    ${response.status_code}

554_01_03 Null Inside Compound Value Rejected On Partial Update
    [Documentation]    5.5.4: the object-nested null exception covers merge
    ...    fragments ONLY — a partial update (5.5.8) carrying one → 400, and
    ...    the stored value stays untouched.
    [Tags]    e-create    e-update    5_5_4    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "address": {"type": "Property", "value": {"street": "Straße des 17. Juni", "country": "Germany"}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${fragment}=    Evaluate
    ...    {"value": {"country": "urn:ngsi-ld:null"}, "@context": [$ngsild_test_suite_context]}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}/attrs/address
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['address']['value']['country']}    Germany
    Should Not Contain    ${response.text}    urn:ngsi-ld:null
    [Teardown]    Delete Entity    ${entity_id}

554_01_04 Merge Fragment Null Removes Key Inside Compound Value
    [Documentation]    5.5.12 exception to 5.5.4: in a merge the patch goes
    ...    into the value object per RFC 7396 — named key updated, null-valued
    ...    key removed, untouched keys preserved; the null sentinel must never
    ...    be stored or served.
    [Tags]    e-create    e-merge    5_5_4    5_5_12    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context], "address": {"type": "Property", "value": {"street": "Straße des 17. Juni", "city": "Berlin", "country": "Germany"}}}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${fragment}=    Evaluate
    ...    {"address": {"type": "Property", "value": {"street": "Pariser Platz", "country": "urn:ngsi-ld:null"}}, "@context": [$ngsild_test_suite_context]}
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${response}=    PATCH
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}${entity_id}
    ...    json=${fragment}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    204    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${value}=    Evaluate    $response.json()['address']['value']
    Should Be Equal    ${value['street']}    Pariser Platz
    Should Be Equal    ${value['city']}    Berlin
    Dictionary Should Not Contain Key    ${value}    country
    Should Not Contain    ${response.text}    urn:ngsi-ld:null
    [Teardown]    Delete Entity    ${entity_id}

554_01_05 Payload That Is Not Valid JSON Is InvalidRequest
    [Documentation]    5.5.4: "If the request payload body is not a valid JSON
    ...    document then an error of type InvalidRequest shall be raised."
    [Tags]    e-create    5_5_4    since_v1.9.1
    &{headers}=    Create Dictionary    Content-Type=application/ld+json
    ${broken}=    Set Variable    {"id": "urn:ngsi-ld:Vehicle:broken", "type":
    ${response}=    POST
    ...    url=${url}/${ENTITIES_ENDPOINT_PATH}
    ...    data=${broken}
    ...    headers=${headers}
    ...    expected_status=any
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_INVALID_REQUEST}
