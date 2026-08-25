*** Settings ***
Documentation       Verify that, when one has an inclusive registration on a Context Broker with redirectionOps, one is able to update entities on a Context Source

Resource            ${EXECDIR}/resources/ApiUtils/Common.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationConsumption.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextInformationProvision.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceDiscovery.resource
Resource            ${EXECDIR}/resources/ApiUtils/ContextSourceRegistration.resource
Resource            ${EXECDIR}/resources/AssertionUtils.resource
Resource            ${EXECDIR}/resources/JsonUtils.resource
Resource            ${EXECDIR}/resources/MockServerUtils.resource

Test Setup          Setup Registration And Start Context Source Mock Server
Test Teardown       Delete Registration And Stop Context Source Mock Server


*** Variables ***
${entity_payload_filename}              vehicle-simple-attributes.jsonld
${registration_payload_file_path}       csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld
${fragment_filename}                    vehicle-brandname-complete-fragment.jsonld


*** Test Cases ***
D004_01_inc Query The Context Broker With Type
    [Documentation]    Check that if one request the Context Broker to update an entity that matches an inclusive registration, this is updated on the Context Source too
    [Tags]    since_v1.6.1    dist-ops    4_3_3    cf_06    additive-inclusive    4_3_6_2    5_6_2

    # ETSI tool bug fixed: the stub path was missing the trailing slash the sibling tests use,
    # so the forwarded PATCH never matched the stub.
    Set Stub Reply    PATCH    /ngsi-ld/v1/entities/${entity_id}/attrs/    204
    ${response}=    Update Entity Attributes
    ...    ${entity_id}
    ...    ${fragment_filename}
    ...    ${CONTENT_TYPE_LD_JSON}

    Wait for redirected request
    Check Response Status Code    207    ${response.status_code}
    ${stub_count}=    Get Stub Count    PATCH    /ngsi-ld/v1/entities/${entity_id}/attrs/
    Should Be True    ${stub_count} > 0
    # ETSI tool bug fixed: the original final assertion evaluated an undefined ${payload}
    # variable (always errored), and iterated entity 'id's over an UpdateResult body
    # (5.2.18: updated/notUpdated attribute names - no entity objects). Removed; the sibling
    # tests (D004_01_exc/_red) also assert status + stub count only.


*** Keywords ***
Setup Registration And Start Context Source Mock Server
    ${entity_id}=    Generate Random Vehicle Entity Id
    Set Suite Variable    ${entity_id}

    ${registration_id}=    Generate Random CSR Id
    Set Suite Variable    ${registration_id}
    ${registration_payload}=    Prepare Context Source Registration From File
    ...    ${registration_id}
    ...    ${registration_payload_file_path}
    ...    entity_id=${entity_id}
    ${response1}=    Create Context Source Registration With Return    ${registration_payload}
    Check Response Status Code    201    ${response1.status_code}
    Start Context Source Mock Server

Delete Registration And Stop Context Source Mock Server
    Delete Context Source Registration    ${registration_id}
    Stop Context Source Mock Server
