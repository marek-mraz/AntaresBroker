*** Settings ***
Documentation       Check that an Attribute name which is a relative-path dot segment is
...                 refused on the temporal Delete Attribute operation.
...
...                 5.6.13.4: "If the target Attribute name is not a valid name, then an
...                 error of type BadRequestData shall be raised." A name valid per 4.6.2
...                 begins with a letter, so neither "." nor ".." is one.
...
...                 The name is interpolated into the request path of the forwarded
...                 operation. IETF RFC 3986 clause 5.2.4 removes dot segments while
...                 resolving that path, so ".." addresses the Temporal Evolution resource
...                 instead of its Attribute and the receiving endpoint executes Delete
...                 Temporal Evolution (5.6.16) in place of Delete Attribute. Percent
...                 triplets are folded before the check because the endpoint decodes the
...                 path it is handed, and a doubly-encoded spelling survives one decode.
...
...                 Only the percent-encoded spellings are exercised: clause 5.2.4 binds
...                 the CLIENT as well, and this suite's HTTP library resolves a raw "."
...                 or ".." before the request leaves, so the broker never receives them
...                 — a raw-segment case would test the client, not the broker. (Verified
...                 on the wire: a request bypassing client resolution gets 400
...                 BadRequestData for ".", ".." and "%2e%2e" alike.)
...
...                 Antares extension TP — 015_02_03 covers only a charset violation
...                 ("invalid(Name"). The entity-side path has this guard; the temporal
...                 path re-implemented the name check inline and dropped it.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/TemporalContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Temporal Entity
Test Teardown       Delete Temporal Entity
Test Template       Delete attribute from temporal entity with a dot segment name


*** Variables ***
${status_code}=     400
${filename}=        vehicle-temporal-representation.jsonld


*** Test Cases ***    ENTITY_ID    ATTRIBUTE_ID
015_04_01 Delete A Temporal Attribute Named With A Percent Encoded Double Dot Segment
    ${valid_temporal_entity_id}    %2e%2e
015_04_02 Delete A Temporal Attribute Named With A Doubly Encoded Double Dot Segment
    ${valid_temporal_entity_id}    %252e%252e
015_04_03 Delete A Temporal Attribute Named With A Percent Encoded Single Dot Segment
    ${valid_temporal_entity_id}    %2e


*** Keywords ***
Delete attribute from temporal entity with a dot segment name
    [Documentation]    5.6.13.4: a dot-segment Attribute name is not a valid name, so the
    ...    operation is BadRequestData and the Temporal Evolution is left intact
    [Tags]    tea-delete    5_6_13    since_v1.9.1
    [Arguments]    ${entity_id}    ${attribute_id}
    ${response}=    Delete Attribute From Temporal Entity
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    content_type=${CONTENT_TYPE_JSON}
    ...    datasetId=${EMPTY}
    ...    deleteAll=false
    Check Response Status Code    ${status_code}    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    ${response.json()}
    ...    ${ERROR_TYPE_BAD_REQUEST_DATA}
    Check Response Body Containing ProblemDetails Element Containing Title Element    ${response.json()}
    # The negative half: a refused name must not have deleted the Temporal Evolution
    # the dot segment resolves to. Without this the test passes on a broker that
    # answered 400 after already performing the wrong operation.
    ${retrieved}=    Retrieve Temporal Representation Of Entity    ${valid_temporal_entity_id}
    Check Response Status Code    200    ${retrieved.status_code}

Create Temporal Entity
    ${valid_temporal_entity_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Or Update Temporal Representation Of Entity Selecting Content Type
    ...    temporal_entity_representation_id=${valid_temporal_entity_id}
    ...    filename=${filename}
    ...    content_type=${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${valid_temporal_entity_id}

Delete Temporal Entity
    Delete Temporal Representation Of Entity    ${valid_temporal_entity_id}
