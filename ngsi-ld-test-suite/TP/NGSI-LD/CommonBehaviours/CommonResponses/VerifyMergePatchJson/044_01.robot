*** Settings ***
Documentation       Verify that PATCH HTTP requests can be done with "application/merge-patch+json" as Content-Type

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource

Test Setup          Create Initial Entity
Test Teardown       Delete Initial Entity


*** Variables ***
${vehicle_filename}=    vehicle-simple-attributes.jsonld
${vehicle_fragment}=    vehicle-brandname-fragment.json
${attribute_id}=        brandName


*** Test Cases ***
044_01_01 Endpoint /entities/{entityId}/attrs/{attrId}
    [Documentation]    Verify that PATCH HTTP requests can be done with "application/merge-patch+json" as Content-Type
    [Tags]    ea-partial-update    cb-mergepatch    6_3_4
    ${response}=    Partial Update Entity Attributes
    ...    entityId=${entity_id}
    ...    attributeId=${attribute_id}
    ...    fragment_filename=${vehicle_fragment}
    ...    content_type=${CONTENT_TYPE_MERGE_PATCH_JSON}
    ...    context=${ngsild_test_suite_context}
    Check Response Status Code    204    ${response.status_code}


*** Keywords ***
Create Initial Entity
    ${entity_id}=    Generate Random Vehicle Entity Id
    ${response}=    Create Entity Selecting Content Type
    ...    ${vehicle_filename}
    ...    ${entity_id}
    ...    ${CONTENT_TYPE_LD_JSON}
    Check Response Status Code    201    ${response.status_code}
    Set Test Variable    ${entity_id}

Delete Initial Entity
    Delete Entity    ${entity_id}
