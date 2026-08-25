*** Settings ***
Documentation       Verify 5.5.7 Term to URI expansion or compaction.
...
...                 5.5.7: the user @context shall not contain JSON-LD Scoped
...                 Contexts (→ BadRequestData); the @context used for
...                 expansion/compaction is the one provided BY EACH API call;
...                 at compaction time an unmatched URI is rendered as the
...                 Fully Qualified Name.
...
...                 Antares extension TP — official TPs cover the embedded
...                 attribute @context arm (451_01) but not scoped contexts
...                 nor the per-request-context EXAMPLE.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource


*** Test Cases ***
557_01_01 Scoped Context In User Context Is Rejected
    [Documentation]    5.5.7: "This user @context shall not contain JSON-LD
    ...    Scoped Contexts" — a term definition carrying its own @context →
    ...    400 BadRequestData, and the entity must NOT exist afterwards.
    [Tags]    e-create    5_5_7    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [{"Vehicle": {"@id": "https://example.org/Vehicle", "@context": {"speed": "https://example.org/hidden-speed"}}}]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    404    ${response.status_code}

557_01_02 Query Uses The Context Of Each Request
    [Documentation]    5.5.7 EXAMPLE: an entity of type "Vehicle" bound to
    ...    @context C matches a query by "Vehicle" iff the query @context maps
    ...    the term to the same URI — a query without that @context expands
    ...    "Vehicle" into the default vocabulary and must NOT return it.
    [Tags]    e-create    e-query    5_5_7    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    # same term, matching @context → found
    ${response}=    Query Entities    entity_types=Vehicle    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e['id'] for e in $response.json()]
    Should Contain    ${ids}    ${entity_id}
    # same term, no @context (core only) → the term maps elsewhere → absent
    ${response}=    Query Entities    entity_types=Vehicle
    Check Response Status Code    200    ${response.status_code}
    ${ids}=    Evaluate    [e['id'] for e in $response.json()]
    Should Not Contain    ${ids}    ${entity_id}
    [Teardown]    Delete Entity    ${entity_id}

557_01_03 Unmatched URI Compacts To The Fully Qualified Name
    [Documentation]    5.5.7: "in the event that no matching term is found in
    ...    the current @context, implementations shall render Fully Qualified
    ...    Names" — retrieving without the creating @context serves the type
    ...    as its full URI, never the bare short name.
    [Tags]    e-create    e-retrieve    5_5_7    since_v1.9.1
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${payload}=    Evaluate
    ...    {"id": $entity_id, "type": "Vehicle", "@context": [$ngsild_test_suite_context]}
    ${response}=    Create Entity From JSON-LD Content    ${payload}
    Check Response Status Code    201    ${response.status_code}
    ${response}=    Retrieve Entity    ${entity_id}
    Check Response Status Code    200    ${response.status_code}
    Should Be Equal    ${response.json()['type']}    https://ngsi-ld-test-suite/context#Vehicle
    [Teardown]    Delete Entity    ${entity_id}
