*** Settings ***
Documentation       Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Teardown       Delete Initial Entity


*** Variables ***
${filename}=    building-simple-attributes.json


*** Test Cases ***
001_04_01 Create One Entity Using A Provided Link Header With JSON Content Type And NGSILD Context
    [Documentation]    Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
    [Tags]    e-create    6_3_5
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ${response1}=    Retrieve Entity    id=${entity_id}    context=${ngsild_test_suite_context}
    # Attribute should be compacted as one used the same context as provided when creating the entity
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=almostFull
    ...    response_body=${response1.json()}

001_04_02 Create One Entity Using A Provided Link Header With JSON Content Type And No Context
    [Documentation]    Check that the @context is obtained from a Link Header if the Content-Type header is "application/json"
    [Tags]    e-create    6_3_5
    ${entity_id}=    Generate Random Building Entity Id
    Set Suite Variable    ${entity_id}
    ${response}=    Create Entity Selecting Content Type
    ...    ${filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_JSON}
    ...    context=${ngsild_test_suite_context}
    ${response1}=    Retrieve Entity    id=${entity_id}
    # Attribute should not be compacted as one did not provide a context containing this term
    Check Response Body Containing an Attribute set to
    ...    expected_attribute_name=https://ngsi-ld-test-suite/context#almostFull
    ...    response_body=${response1.json()}


*** Keywords ***
Delete Initial Entity
    Delete Entity    ${entity_id}
