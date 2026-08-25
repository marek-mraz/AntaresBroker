*** Settings ***
Documentation       Check that the default @context is used if the Content-Type header is "application/json" and the Link header does not contain a JSON-LD @context

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=    building-simple-attributes.json


*** Test Cases ***
001_05_01 Create One Entity Using The Default Context With JSON Content Type And Request Without Context
    [Documentation]    Check that the default @context is used if the Content-Type header is "application/json" and the Link header does not contain a JSON-LD @context, requesting without context
    [Tags]    e-create    6_3_5
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type    ${filename}    ${entity_id}    ${CONTENT_TYPE_JSON}
    ${response1}=    Retrieve Entity    id=${entity_id}
    # Attribute should be compacted as one used the same default context as provided when creating the entity
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=almostFull
    ...    response_body=${response1.json()}

001_05_02 Create One Entity Using The Default Context With JSON Content Type And Request With Context
    [Documentation]    Check that the default @context is used if the Content-Type header is "application/json" and the Link header does not contain a JSON-LD @context, requesting with context
    [Tags]    e-create    6_3_5
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type    ${filename}    ${entity_id}    ${CONTENT_TYPE_JSON}
    ${response1}=    Retrieve Entity
    ...    id=${entity_id}
    ...    accept=${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    # Attribute should not be compacted as one did not provide a context containing this term
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=ngsi-ld:default-context/almostFull
    ...    response_body=${response1.json()}


*** Keywords ***
Delete Initial Entity
    Delete Entity    ${entity_id}
