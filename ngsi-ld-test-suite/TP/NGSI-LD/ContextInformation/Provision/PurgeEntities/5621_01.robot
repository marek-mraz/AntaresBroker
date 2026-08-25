*** Settings ***
Documentation       Verify 5.6.21.4 Purge Entities rejection of Linked Entity
...                 paths, an edge the official 059/060 TPs skip.
...
...                 5.6.21.4: "If projection attributes are present and
...                 indicate the use of Linked Entity retrieval, then an
...                 error of type BadRequestData shall be raised. If the
...                 filter conditions specified by the query includes Linked
...                 Entity attributes then an error of type BadRequestData
...                 shall be raised." The entity must survive the rejected
...                 purge.
...
...                 Antares extension TP.

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource

Test Setup          Setup Initial Entity
Test Teardown       Delete Entity    ${entity_id}


*** Test Cases ***
5621_01_01 Purge With Linked Entity Query Is BadRequestData
    [Documentation]    5.6.21.4: q using the 4.9 LinkedEntityRelation form
    ...    (owner{name}) raises BadRequestData; nothing is deleted.
    [Tags]    e-purge    5_6_21    since_v1.9.1
    ${response}=    Purge Entities
    ...    type=Building
    ...    q=owner{name}=="x"
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}
    ${retrieve_response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${retrieve_response.status_code}

5621_01_02 Purge With Linked Entity Projection Is BadRequestData
    [Documentation]    5.6.21.4: projection attributes indicating Linked
    ...    Entity retrieval (attrs=owner{name}) raise BadRequestData;
    ...    nothing is deleted.
    [Tags]    e-purge    5_6_21    since_v1.9.1
    ${response}=    Purge Entities
    ...    type=Building
    ...    attrs=owner{name}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    400    ${response.status_code}
    Check Response Body Containing ProblemDetails Element Containing Type Element set to
    ...    response_body=${response.json()}
    ...    type=${ERROR_TYPE_BAD_REQUEST_DATA}
    ${retrieve_response}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    Check Response Status Code    200    ${retrieve_response.status_code}


*** Keywords ***
Setup Initial Entity
    ${entity_id}=    Catenate    ${BUILDING_ID_PREFIX}5621-01-1
    Set Test Variable    ${entity_id}
    ${create_response}=    Create Entity Selecting Content Type
    ...    building-simple-attributes.jsonld
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${create_response.status_code}
